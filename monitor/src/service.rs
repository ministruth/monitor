use std::{cmp::max, net::SocketAddr};

use actix_cloud::{
    chrono::Utc,
    tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver},
};
use ecies::SecretKey;
use itertools::Itertools;
use miniz_oxide::deflate::compress_to_vec;
use once_cell::sync::Lazy;
use serde_json::Value;
use skynet_api::{
    ffi_rpc::{self, async_trait, bincode, ffi_rpc_macro::plugin_impl_trait, registry::Registry},
    sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseTransaction, EntityTrait, Set},
    service::{SError, SResult},
    viewer::settings::SettingViewer,
    HyUuid, Result,
};
use skynet_api_monitor::{
    entity::agents, message::Data, semver::Version, viewer::agents::AgentViewer, Agent,
    AgentCommand, AgentFile, AgentStatus, CommandKillMessage, CommandReqMessage, FileReqMessage,
    InfoMessage, StatusRspMessage, ID,
};

use crate::{Plugin, PLUGIN_INSTANCE};

static SETTING_ADDRESS: Lazy<String> = Lazy::new(|| format!("plugin_{ID}_address"));
static SETTING_CERTIFICATE: Lazy<String> = Lazy::new(|| format!("plugin_{ID}_certificate"));
static SETTING_SHELL: Lazy<String> = Lazy::new(|| format!("plugin_{ID}_shell"));

#[plugin_impl_trait]
impl skynet_api_monitor::Service for Plugin {
    async fn api_version(&self, _: &Registry) -> Version {
        Version::parse(skynet_api_monitor::VERSION).unwrap()
    }

    async fn get_agents(&self, _: &Registry) -> Vec<Agent> {
        self.agent.iter().map(|x| x.value().to_owned()).collect()
    }

    async fn find_agent(&self, _: &Registry, id: HyUuid) -> Option<Agent> {
        self.agent.get(&id).map(|x| x.value().to_owned())
    }

    async fn run_command(&self, _: &Registry, id: HyUuid, cmd: String) -> SResult<HyUuid> {
        if let Some(mut x) = self.agent.get_mut(&id) {
            if let Some(msg) = &x.message {
                let id = HyUuid::new();
                msg.send(Data::CommandReq(CommandReqMessage {
                    id: id.to_string(),
                    cmd: cmd.to_owned(),
                }))?;
                x.command.insert(id, None);
                return Ok(id);
            }
        }
        Err(SError::new("Agent not exist or offline"))
    }

    /// Get agent `id` command `cid` output.
    async fn get_command_output(
        &self,
        _: &Registry,
        id: HyUuid,
        cid: HyUuid,
    ) -> Option<AgentCommand> {
        self.agent.get(&id)?.command.get(&cid)?.to_owned()
    }

    /// Kill async command `cid` in agent `id`.
    async fn kill_command(
        &self,
        _: &Registry,
        id: HyUuid,
        cid: HyUuid,
        force: bool,
    ) -> SResult<()> {
        if let Some(x) = self.agent.get(&id) {
            if let Some(x) = &x.message {
                return x
                    .send(Data::CommandKill(CommandKillMessage {
                        id: cid.to_string(),
                        force,
                    }))
                    .map_err(Into::into);
            }
        }
        Err(SError::new("Agent not exist or offline"))
    }

    /// Send file to agent `id`.
    /// File contents will be compressed automatically.
    ///
    /// Return file id when success.
    async fn send_file(
        &self,
        _: &Registry,
        id: HyUuid,
        path: String,
        data: Vec<u8>,
    ) -> SResult<HyUuid> {
        if let Some(mut x) = self.agent.get_mut(&id) {
            if let Some(msg) = &x.message {
                let id = HyUuid::new();
                let data = compress_to_vec(&data, 6);
                msg.send(Data::FileReq(FileReqMessage {
                    id: id.to_string(),
                    path: path.to_owned(),
                    data,
                }))?;
                x.file.insert(id, None);
                return Ok(id);
            }
        }
        Err(SError::new("Agent not exist or offline"))
    }

    async fn get_file_result(&self, _: &Registry, id: HyUuid, fid: HyUuid) -> Option<AgentFile> {
        self.agent.get(&id)?.file.get(&fid)?.to_owned()
    }
}

impl Plugin {
    /// Login agent `uid` with `ip`. Returns `None` when already login, otherwise agent id.
    pub async fn login(
        &self,
        db: &DatabaseTransaction,
        uid: &str,
        addr: &SocketAddr,
    ) -> Result<Option<HyUuid>> {
        let ip = addr.ip().to_string();
        let agent = AgentViewer::find_by_uid(db, uid).await?;
        let now = Utc::now().timestamp_millis();
        let agent = if let Some(agent) = agent {
            agent
        } else {
            agents::ActiveModel {
                uid: Set(uid.to_owned()),
                name: Set(uid.chars().take(8).collect()),
                ip: Set(ip.clone()),
                last_login: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?
        };
        let status = self.agent.get(&agent.id).map(|x| x.status);
        if let Some(status) = status {
            if status.is_offline() {
                let mut agent: agents::ActiveModel = agent.into();
                agent.ip = Set(ip.clone());
                agent.last_login = Set(now);
                let agent = agent.update(db).await?;

                Ok(Some(
                    self.agent
                        .get_mut(&agent.id)
                        .map(|mut x| {
                            x.ip = ip;
                            x.last_login = now;
                            x.status = AgentStatus::Online;
                            x.address = Some(*addr);
                            agent.id
                        })
                        .unwrap(),
                ))
            } else {
                Ok(None)
            }
        } else {
            let mut agent: Agent = agent.into();
            agent.status = AgentStatus::Online;
            agent.address = Some(*addr);
            let id = agent.id;
            self.agent.insert(id, agent);
            Ok(Some(id))
        }
    }

    /// Logout agent `id`. Will be invoked automatically when connection losts.
    pub fn logout(&self, id: &HyUuid) {
        if let Some(mut item) = self.agent.get_mut(id) {
            item.status = AgentStatus::Offline;
            item.endpoint.clear();
            item.address = None;
            item.disable_shell = false;
            item.report_rate = 0;
            item.last_rsp = None;
            item.cpu = None;
            item.memory = None;
            item.total_memory = None;
            item.disk = None;
            item.total_disk = None;
            item.latency = None;
            item.net_up = None;
            item.net_down = None;
            item.band_up = None;
            item.band_down = None;
            item.message = None;
        }
    }

    /// Update agent `id` status.
    pub fn update_status(&self, id: &HyUuid, data: StatusRspMessage) {
        let now = Utc::now().timestamp_millis();
        if let Some(mut item) = self.agent.get_mut(id) {
            if let Some(rsp) = item.last_rsp {
                if let Some(x) = item.band_up {
                    item.net_up = Some((data.band_up - x) * 1000 / max(now - rsp, 1) as u64);
                }
                if let Some(x) = item.band_down {
                    item.net_down = Some((data.band_down - x) * 1000 / max(now - rsp, 1) as u64);
                }
            }

            item.last_rsp = Some(now);
            item.cpu = Some(data.cpu);
            item.memory = Some(data.memory);
            item.total_memory = Some(data.total_memory);
            item.disk = Some(data.disk);
            item.total_disk = Some(data.total_disk);
            item.latency = Some((now - data.time) / 2); // round trip
            item.band_up = Some(data.band_up);
            item.band_down = Some(data.band_down);
        }
    }

    pub async fn update_agent<C>(&self, db: &C, id: &HyUuid, data: InfoMessage) -> Result<()>
    where
        C: ConnectionTrait,
    {
        AgentViewer::update(db, id, &data).await?;
        if let Some(mut item) = self.agent.get_mut(id) {
            item.os = data.os;
            item.system = data.system;
            item.arch = data.arch;
            item.hostname = data.hostname;
            if let Some(ip) = data.ip {
                item.ip = ip;
            }
            item.endpoint = data.endpoint;
            item.disable_shell = data.disable_shell;
            item.report_rate = data.report_rate;
        }
        Ok(())
    }

    /// Bind message channel.
    pub fn bind_message(&self, id: &HyUuid) -> UnboundedReceiver<Data> {
        let (tx, rx) = unbounded_channel();
        if let Some(mut item) = self.agent.get_mut(id) {
            item.message = Some(tx);
        }
        rx
    }

    /// Update agent `id` command `cid` code and output.
    ///
    /// Return true when `id` and `cid` is valid.
    pub fn update_command_output(
        &self,
        id: &HyUuid,
        cid: &HyUuid,
        code: Option<i32>,
        mut output: Vec<u8>,
    ) -> bool {
        if let Some(mut agent) = self.agent.get_mut(id) {
            if let Some(command) = agent.command.get_mut(cid) {
                if command.is_none() {
                    *command = Some(AgentCommand::new());
                }
                command.as_mut().unwrap().code = code;
                command.as_mut().unwrap().output.append(&mut output);
                return true;
            }
        }
        false
    }

    /// Update agent `id` file `mid` code and message.
    ///
    /// Return true when `id` and `mid` is valid.
    pub fn update_file_response(
        &self,
        id: &HyUuid,
        fid: &HyUuid,
        code: u32,
        message: &str,
    ) -> bool {
        if let Some(mut agent) = self.agent.get_mut(id) {
            if let Some(file) = agent.file.get_mut(fid) {
                if file.is_none() {
                    *file = Some(AgentFile::new());
                }
                file.as_mut().unwrap().code = code;
                file.as_mut().unwrap().message = message.to_owned();
                return true;
            }
        }
        false
    }

    pub async fn get_setting_address<C>(db: &C) -> Result<Option<String>>
    where
        C: ConnectionTrait,
    {
        SettingViewer::get(db, &SETTING_ADDRESS).await
    }

    pub async fn get_setting_certificate<C>(db: &C) -> Result<Option<SecretKey>>
    where
        C: ConnectionTrait,
    {
        Ok(SettingViewer::get_base64(db, &SETTING_CERTIFICATE)
            .await?
            .and_then(|d| d.try_into().ok().and_then(|d| SecretKey::parse(&d).ok())))
    }

    pub async fn get_setting_shell<C>(db: &C) -> Result<Option<Vec<String>>>
    where
        C: ConnectionTrait,
    {
        if let Some(x) = SettingViewer::get(db, &SETTING_SHELL).await? {
            if let Ok(x) = serde_json::from_str::<Value>(&x) {
                return Ok(x.as_array().map(|x| {
                    x.iter()
                        .map(|x| x.as_str().unwrap_or("").to_owned())
                        .unique()
                        .filter(|x| !x.is_empty())
                        .collect()
                }));
            }
        }
        Ok(None)
    }

    pub async fn set_setting_address(db: &DatabaseTransaction, address: &str) -> Result<()> {
        SettingViewer::set(db, &SETTING_ADDRESS, address).await
    }

    pub async fn set_setting_certificate(db: &DatabaseTransaction, cert: &SecretKey) -> Result<()> {
        SettingViewer::set_base64(db, &SETTING_CERTIFICATE, &cert.serialize()).await
    }

    pub async fn set_setting_shell(db: &DatabaseTransaction, shell_prog: &[String]) -> Result<()> {
        SettingViewer::set(db, &SETTING_SHELL, &serde_json::to_string(&shell_prog)?).await
    }

    pub async fn init_agent(&self, db: &DatabaseTransaction) -> Result<()> {
        agents::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(From::from)
            .for_each(|x: Agent| {
                self.agent.insert(x.id, x);
            });
        Ok(())
    }
}
