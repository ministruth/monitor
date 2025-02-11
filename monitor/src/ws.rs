use std::sync::Arc;

use actix_cloud::actix_web::{web, HttpResponse};
use bytes::Bytes;
use skynet_api::{
    anyhow::anyhow, bail, ffi_rpc::registry::Registry, plugin::WSMessage, request::Request,
    service::SKYNET_SERVICE, tracing::debug, HyUuid, Result,
};
use skynet_api_monitor::{
    frontend_message::Data, message, prost::Message as _, FrontendMessage, ShellDisconnectMessage,
    ShellErrorMessage,
};

use crate::PLUGIN_INSTANCE;

pub struct ShellService {
    id: HyUuid,
    agent_id: Option<HyUuid>,
    token: HyUuid,
    skynet: Arc<skynet_api::service::Service>,
    reg: Arc<Registry>,
}

impl Drop for ShellService {
    fn drop(&mut self) {
        self.cleanup();
    }
}

impl ShellService {
    pub fn new(skynet: skynet_api::service::Service, reg: Arc<Registry>, id: &HyUuid) -> Self {
        Self {
            token: HyUuid::nil(),
            skynet: skynet.into(),
            reg,
            id: *id,
            agent_id: None,
        }
    }

    pub async fn send(&self, data: FrontendMessage) -> Result<()> {
        self.skynet
            .websocket_send(
                self.reg.as_ref(),
                &self.id,
                &WSMessage::Binary(data.encode_to_vec().into()),
            )
            .await
            .map_err(Into::into)
    }

    fn cleanup(&mut self) {
        if self.agent_id.is_some() {
            let _ = self.send_agent(message::Data::ShellDisconnect(ShellDisconnectMessage {
                token: Some(self.token.to_string()),
            }));
            debug!(trace_id = ?self.id, "Websocket cleanup");
        }
        self.agent_id = None;
        self.token = HyUuid::nil();
        PLUGIN_INSTANCE.shell_binding.remove(&self.token);
    }

    fn send_agent(&self, data: message::Data) -> Result<()> {
        if let Some(id) = &self.agent_id {
            if let Some(agent) = PLUGIN_INSTANCE.agent.get(id) {
                if !agent.disable_shell {
                    let c = agent.message.as_ref().ok_or(anyhow!("Agent is offline"))?;
                    c.send(data)?;
                }
                Ok(())
            } else {
                bail!("Agent does not exist")
            }
        } else {
            bail!("Shell does not connect")
        }
    }

    fn recv(&mut self, text: Bytes) -> Result<()> {
        let msg = FrontendMessage::decode(text)?;
        if let Some(data) = msg.data {
            match data {
                Data::ShellConnect(data) => {
                    self.cleanup();
                    let token = HyUuid::parse(&data.token)?;
                    PLUGIN_INSTANCE.shell_binding.insert(token, self.id);
                    self.agent_id =
                        Some(HyUuid::parse(&msg.id.ok_or(anyhow!("Invalid message"))?)?);
                    self.token = token;
                    debug!(trace_id = ?self.id, "Websocket shell connect");
                    self.send_agent(message::Data::ShellConnect(data))
                }
                Data::ShellInput(mut data) => {
                    data.token = Some(self.token.to_string());
                    self.send_agent(message::Data::ShellInput(data))
                }
                Data::ShellResize(mut data) => {
                    data.token = Some(self.token.to_string());
                    self.send_agent(message::Data::ShellResize(data))
                }
                Data::ShellDisconnect(_) => {
                    self.cleanup();
                    Ok(())
                }
                _ => bail!("Invalid message type"),
            }
        } else {
            bail!("Invalid data")
        }
    }
}

pub async fn service(reg: web::Data<Registry>, req: Request, data: WSMessage) -> HttpResponse {
    let id = req.trace_id();
    match data {
        WSMessage::Connect => {
            let skynet: skynet_api::service::Service = reg.get(SKYNET_SERVICE).unwrap().into();
            PLUGIN_INSTANCE
                .shell
                .insert(id, ShellService::new(skynet, reg.into_inner(), &id));
        }
        WSMessage::Binary(s) => {
            let skynet: skynet_api::service::Service = reg.get(SKYNET_SERVICE).unwrap().into();
            if let Some(mut x) = PLUGIN_INSTANCE.shell.get_mut(&id) {
                if let Err(e) = x.recv(s) {
                    let _ = skynet
                        .websocket_send(
                            &reg,
                            &id,
                            &WSMessage::Binary(
                                FrontendMessage {
                                    id: None,
                                    data: Some(Data::ShellError(ShellErrorMessage {
                                        token: None,
                                        error: e.to_string(),
                                    })),
                                }
                                .encode_to_vec()
                                .into(),
                            ),
                        )
                        .await;
                    debug!(error = %e, "Error handle ws message");
                }
            } else {
                skynet.websocket_close(&reg, &id).await;
            }
        }
        WSMessage::Close => {
            PLUGIN_INSTANCE.shell.remove(&id);
        }
        _ => (),
    };
    HttpResponse::Ok().finish()
}
