use core::str;
use std::time::Duration;

use actix_cloud::{
    actix_web::{HttpResponse, web::Path},
    response::{JsonResponse, RspResult},
    tokio::{spawn, time::sleep},
    tracing::{error, info},
};
use actix_web_validator::{Json, QsQuery};
use base64::{Engine, engine::general_purpose::STANDARD};
use ecies::{PublicKey, utils::generate_keypair};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_repr::Serialize_repr;
use skynet_api::{
    HyUuid, Result, finish,
    request::{
        Condition, IDsReq, IntoExpr, PageData, PaginationParam, TimeParam, unique_validator,
    },
    sea_orm::{ColumnTrait, IntoSimpleExpr, TransactionTrait},
};
use skynet_api_monitor::{
    AgentStatus, ReconnectMessage,
    entity::passive_agents,
    viewer::{agents::AgentViewer, passive_agents::PassiveAgentViewer},
};
use skynet_macro::common_req;
use validator::Validate;

use crate::{MonitorResponse, PLUGIN_INSTANCE, Plugin};

#[derive(Debug, Validate, Deserialize)]
pub struct GetAgentsReq {
    #[validate(custom(function = "unique_validator"))]
    status: Option<Vec<AgentStatus>>,
    text: Option<String>,

    #[serde(flatten)]
    #[validate(nested)]
    page: PaginationParam,
}

pub async fn get_agents(param: QsQuery<GetAgentsReq>) -> RspResult<JsonResponse> {
    let data: Vec<serde_json::Value> = PLUGIN_INSTANCE
        .agent
        .iter()
        .filter(|v| {
            if let Some(x) = &param.status {
                if !x.contains(&v.status) {
                    return false;
                }
            }
            if let Some(x) = &param.text {
                if !v.id.to_string().contains(x)
                    && !v.name.contains(x)
                    && !v.ip.contains(x)
                    && !v.os.as_ref().is_some_and(|v| v.contains(x))
                    && !v.arch.as_ref().is_some_and(|v| v.contains(x))
                {
                    return false;
                }
            }
            true
        })
        .map(|x| json!(x.value()))
        .collect();
    finish!(JsonResponse::new(MonitorResponse::Success).json(param.page.split(data)));
}

#[common_req(passive_agents::Column)]
#[derive(Debug, Validate, Deserialize)]
pub struct GetPassiveAgentsReq {
    pub text: Option<String>,

    #[serde(flatten)]
    #[validate(nested)]
    pub page: PaginationParam,
    #[serde(flatten)]
    #[validate(nested)]
    pub time: TimeParam,
}

pub async fn get_passive_agents(param: QsQuery<GetPassiveAgentsReq>) -> RspResult<JsonResponse> {
    #[derive(Serialize_repr)]
    #[repr(u8)]
    enum Status {
        Inactive = 0,
        Active,
    }
    #[derive(Serialize)]
    struct Rsp {
        id: HyUuid,
        name: String,
        status: Status,
        address: String,
        retry_time: i32,
        created_at: i64,
        updated_at: i64,
    }
    let mut cond = param.common_cond();
    if let Some(text) = &param.text {
        cond = cond.add(
            Condition::any()
                .add(text.like_expr(passive_agents::Column::Id))
                .add(text.like_expr(passive_agents::Column::Name))
                .add(text.like_expr(passive_agents::Column::Address)),
        );
    }
    let data = PassiveAgentViewer::find(PLUGIN_INSTANCE.db.get().unwrap(), cond).await?;
    let agent = PLUGIN_INSTANCE.server.connecting();
    let data = (
        data.0
            .into_iter()
            .map(|x| Rsp {
                id: x.id,
                name: x.name,
                status: if agent.contains(&x.id) {
                    Status::Active
                } else {
                    Status::Inactive
                },
                address: x.address,
                retry_time: x.retry_time,
                created_at: x.created_at,
                updated_at: x.updated_at,
            })
            .collect(),
        data.1,
    );

    finish!(JsonResponse::new(MonitorResponse::Success).json(PageData::new(data)));
}

#[derive(Debug, Validate, Deserialize)]
pub struct AddPassiveAgentsReq {
    #[validate(length(min = 1, max = 32))]
    pub name: String,
    #[validate(length(min = 1, max = 64))]
    pub address: String,
    #[validate(range(min = 0))]
    pub retry_time: i32,
}

pub async fn add_passive_agents(param: Json<AddPassiveAgentsReq>) -> RspResult<JsonResponse> {
    let tx = PLUGIN_INSTANCE.db.get().unwrap().begin().await?;
    if PassiveAgentViewer::find_by_name(&tx, &param.address)
        .await?
        .is_some()
    {
        finish!(JsonResponse::new(MonitorResponse::PassiveAgentNameExist));
    }
    if PassiveAgentViewer::find_by_address(&tx, &param.address)
        .await?
        .is_some()
    {
        finish!(JsonResponse::new(MonitorResponse::PassiveAgentAddressExist));
    }
    let m = PassiveAgentViewer::create(&tx, &param.name, &param.address, param.retry_time).await?;
    tx.commit().await?;
    PLUGIN_INSTANCE.server.connect(&m.id);

    info!(
        success = true,
        name = param.name,
        address = param.address,
        retry_time = param.retry_time,
        "Add passive agent",
    );
    finish!(JsonResponse::new(MonitorResponse::Success).json(m.id));
}

#[derive(Debug, Validate, Deserialize)]
pub struct PutPassiveAgentsReq {
    #[validate(length(min = 1, max = 32))]
    pub name: Option<String>,
    #[validate(length(min = 1, max = 64))]
    pub address: Option<String>,
    #[validate(range(min = 0))]
    pub retry_time: Option<i32>,
}

pub async fn put_passive_agents(
    paid: Path<HyUuid>,
    param: Json<PutPassiveAgentsReq>,
) -> RspResult<JsonResponse> {
    let tx = PLUGIN_INSTANCE.db.get().unwrap().begin().await?;
    if PassiveAgentViewer::find_by_id(&tx, &paid).await?.is_none() {
        finish!(JsonResponse::not_found());
    }
    PassiveAgentViewer::update(
        &tx,
        &paid,
        param.name.as_deref(),
        param.address.as_deref(),
        param.retry_time,
    )
    .await?;
    tx.commit().await?;

    info!(
        success = true,
        paid = %paid,
        name = ?param.name,
        address = ?param.address,
        retry_time = ?param.retry_time,
        "Put passive agent",
    );
    finish!(JsonResponse::new(MonitorResponse::Success))
}

pub async fn activate_passive_agents(paid: Path<HyUuid>) -> RspResult<JsonResponse> {
    if PassiveAgentViewer::find_by_id(PLUGIN_INSTANCE.db.get().unwrap(), &paid)
        .await?
        .is_none()
    {
        finish!(JsonResponse::not_found());
    }
    PLUGIN_INSTANCE.server.connect(&paid);

    info!(
        success = true,
        paid = %paid,
        "Activate passive agent",
    );
    finish!(JsonResponse::new(MonitorResponse::Success))
}

pub async fn delete_passive_agents_batch(param: Json<IDsReq>) -> RspResult<JsonResponse> {
    let rows = PassiveAgentViewer::delete(PLUGIN_INSTANCE.db.get().unwrap(), &param.id).await?;
    if rows != 0 {
        info!(
            success = true,
            paid = ?param.id,
            "Delete passive agents",
        );
    }
    finish!(JsonResponse::new(MonitorResponse::Success).json(rows));
}

pub async fn delete_passive_agents(paid: Path<HyUuid>) -> RspResult<JsonResponse> {
    let rows = PassiveAgentViewer::delete(PLUGIN_INSTANCE.db.get().unwrap(), &[*paid]).await?;
    info!(
        success = true,
        paid = %paid,
        "Delete passive agent",
    );
    finish!(JsonResponse::new(MonitorResponse::Success).json(rows));
}

pub async fn get_settings() -> RspResult<JsonResponse> {
    #[derive(Serialize)]
    struct Rsp {
        running: bool,
        shell: Vec<String>,
        address: String,
        msg_timeout: u32,
        alert_timeout: u32,
    }

    let db = PLUGIN_INSTANCE.db.get().unwrap();
    finish!(
        JsonResponse::new(MonitorResponse::Success).json(Rsp {
            running: PLUGIN_INSTANCE.server.is_running(),
            shell: Plugin::get_setting_shell(db).await?.unwrap_or_default(),
            address: Plugin::get_setting_address(db).await?.unwrap_or_default(),
            msg_timeout: Plugin::get_setting_msg_timeout(db)
                .await?
                .unwrap_or_default(),
            alert_timeout: Plugin::get_setting_alert_timeout(db)
                .await?
                .unwrap_or_default(),
        })
    );
}

pub async fn get_settings_shell() -> RspResult<JsonResponse> {
    finish!(
        JsonResponse::new(MonitorResponse::Success).json(
            Plugin::get_setting_shell(PLUGIN_INSTANCE.db.get().unwrap())
                .await?
                .unwrap_or_default()
        )
    );
}

pub async fn get_settings_certificate() -> RspResult<HttpResponse> {
    let pk = PublicKey::from_secret_key(
        &Plugin::get_setting_certificate(PLUGIN_INSTANCE.db.get().unwrap())
            .await?
            .unwrap_or_default(),
    );
    finish!(JsonResponse::file(
        String::from("pubkey"),
        STANDARD.encode(pk.serialize()).into()
    ))
}

async fn restart_server(max_time: u32) -> Result<()> {
    let db = PLUGIN_INSTANCE.db.get().unwrap();
    let addr = Plugin::get_setting_address(db).await?.unwrap_or_default();
    let key = Plugin::get_setting_certificate(db)
        .await?
        .unwrap_or_default();
    let srv = &PLUGIN_INSTANCE.server;
    srv.stop();
    for _ in 0..max_time {
        if !srv.is_running() {
            break;
        }
        sleep(Duration::from_secs(1)).await;
    }
    if !srv.is_running() {
        spawn(async move {
            srv.start(&addr, key)
                .await
                .map_err(|e| error!(address=addr, error=%e, "Failed to start server"))
        });
    }
    Ok(())
}

pub async fn new_settings_certificate() -> RspResult<JsonResponse> {
    let key = generate_keypair();
    let tx = PLUGIN_INSTANCE.db.get().unwrap().begin().await?;
    Plugin::set_setting_certificate(&tx, &key.0).await?;
    tx.commit().await?;

    restart_server(5).await?;

    info!(success = true, "New monitor certificate",);
    finish!(JsonResponse::new(MonitorResponse::Success))
}

#[derive(Debug, Validate, Deserialize)]
pub struct PostServerReq {
    pub start: bool,
}

pub async fn post_server(param: Json<PostServerReq>) -> RspResult<JsonResponse> {
    let srv = &PLUGIN_INSTANCE.server;
    if param.start {
        if !srv.is_running() {
            let db = PLUGIN_INSTANCE.db.get().unwrap();
            let addr = Plugin::get_setting_address(db).await?.unwrap_or_default();
            let key = Plugin::get_setting_certificate(db)
                .await?
                .unwrap_or_default();
            spawn(async move {
                srv.start(&addr, key)
                    .await
                    .map_err(|e| error!(address=addr, error=%e, "Failed to start server"))
            });
        }
    } else if srv.is_running() {
        srv.stop();
    }
    info!(success = true, start = param.start, "Post monitor server",);
    finish!(JsonResponse::new(MonitorResponse::Success))
}

#[derive(Debug, Validate, Deserialize)]
pub struct PutSettingsReq {
    #[validate(custom(function = "unique_validator"))]
    pub shell: Option<Vec<String>>,
    pub address: Option<String>,
    pub msg_timeout: Option<u32>,
    pub alert_timeout: Option<u32>,
}

pub async fn put_settings(param: Json<PutSettingsReq>) -> RspResult<JsonResponse> {
    let tx = PLUGIN_INSTANCE.db.get().unwrap().begin().await?;
    if let Some(x) = &param.shell {
        Plugin::set_setting_shell(&tx, x).await?;
    }
    if let Some(x) = &param.address {
        Plugin::set_setting_address(&tx, x).await?;
    }
    if let Some(x) = &param.msg_timeout {
        Plugin::set_setting_msg_timeout(&tx, *x).await?;
        *PLUGIN_INSTANCE.msg_timeout.write() = *x;
    }
    if let Some(x) = &param.alert_timeout {
        Plugin::set_setting_alert_timeout(&tx, *x).await?;
        *PLUGIN_INSTANCE.alert_timeout.write() = *x;
    }
    tx.commit().await?;

    if param.address.is_some() {
        restart_server(5).await?;
    }

    info!(
        success = true,
        address = ?param.address,
        shell = ?param.shell,
        "Put monitor settings",
    );
    finish!(JsonResponse::new(MonitorResponse::Success))
}

pub async fn reconnect_agent(aid: Path<HyUuid>) -> RspResult<JsonResponse> {
    if let Some(agent) = PLUGIN_INSTANCE.agent.get(&aid) {
        if let Some(x) = &agent.message {
            x.send(skynet_api_monitor::message::Data::Reconnect(
                ReconnectMessage {},
            ))?;
        }
    } else {
        finish!(JsonResponse::not_found());
    }
    info!(
        success = true,
        aid = %aid,
        "Reconnect monitor agent",
    );
    finish!(JsonResponse::new(MonitorResponse::Success))
}

#[derive(Debug, Validate, Deserialize)]
pub struct PutAgentsReq {
    #[validate(length(max = 32))]
    name: String,
}

pub async fn put_agent(aid: Path<HyUuid>, param: Json<PutAgentsReq>) -> RspResult<JsonResponse> {
    if PLUGIN_INSTANCE.agent.get(&aid).is_none() {
        finish!(JsonResponse::not_found());
    }

    let tx = PLUGIN_INSTANCE.db.get().unwrap().begin().await?;
    if AgentViewer::find_by_name(&tx, &param.name).await?.is_some() {
        finish!(JsonResponse::new(MonitorResponse::AgentExist));
    }
    AgentViewer::rename(&tx, &aid, &param.name).await?;
    if let Some(mut x) = PLUGIN_INSTANCE.agent.get_mut(&aid) {
        x.name = param.name.clone();
    }
    tx.commit().await?;

    info!(
        success = true,
        aid = %aid,
        name = param.name,
        "Put monitor agent",
    );
    finish!(JsonResponse::new(MonitorResponse::Success))
}

pub async fn delete_agent(aid: Path<HyUuid>) -> RspResult<JsonResponse> {
    if PLUGIN_INSTANCE.agent.get(&aid).is_none() {
        finish!(JsonResponse::not_found());
    }

    let tx = PLUGIN_INSTANCE.db.get().unwrap().begin().await?;
    let rows = AgentViewer::delete(&tx, &[*aid]).await?;
    PLUGIN_INSTANCE.remove_agent(&aid);
    tx.commit().await?;

    info!(
        success = true,
        aid = %aid,
        "Delete monitor agent",
    );
    finish!(JsonResponse::new(MonitorResponse::Success).json(rows))
}

pub async fn delete_agents(param: Json<IDsReq>) -> RspResult<JsonResponse> {
    let tx = PLUGIN_INSTANCE.db.get().unwrap().begin().await?;
    let rows = AgentViewer::delete(&tx, &param.id).await?;
    for i in &param.id {
        PLUGIN_INSTANCE.remove_agent(i);
    }
    tx.commit().await?;

    info!(
        success = true,
        aid = ?param.id,
        "Delete monitor agents",
    );
    finish!(JsonResponse::new(MonitorResponse::Success).json(rows))
}
