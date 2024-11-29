use actix_cloud::{tokio::sync::mpsc::UnboundedSender, utils};
use derivative::Derivative;
use entity::agents;
use enum_as_inner::EnumAsInner;
use ffi_rpc::{
    self, abi_stable, async_trait, bincode,
    ffi_rpc_macro::{self, plugin_api},
};
use message::Data;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use skynet_api::{service::SResult, uuid, HyUuid};
use std::{collections::HashMap, net::SocketAddr};

pub use prost;
pub use semver;
pub mod entity;
pub mod viewer;
include!(concat!(env!("OUT_DIR"), "/msg.rs"));

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ID: HyUuid = HyUuid(uuid!("2eb2e1a5-66b4-45f9-ad24-3c4f05c858aa"));

#[plugin_api(MonitorService)]
pub trait Service {
    async fn api_version() -> Version;

    async fn get_agents() -> Vec<Agent>;

    async fn find_agent(id: HyUuid) -> Option<Agent>;

    /// Run async command `cmd` in agent `id`. Return generated command id.
    async fn run_command(id: HyUuid, cmd: String) -> SResult<HyUuid>;

    /// Get agent `id` command `cid` output.
    async fn get_command_output(id: HyUuid, cid: HyUuid) -> Option<AgentCommand>;

    /// Kill async command `cid` in agent `id`.
    async fn kill_command(id: HyUuid, cid: HyUuid, force: bool) -> SResult<()>;

    /// Send file to agent `id`.
    /// File contents will be compressed automatically.
    ///
    /// Return file id when success.
    async fn send_file(id: HyUuid, path: String, data: Vec<u8>) -> SResult<HyUuid>;

    async fn get_file_result(id: HyUuid, fid: HyUuid) -> Option<AgentFile>;
}

#[derive(
    Default, EnumAsInner, Debug, Serialize_repr, Deserialize_repr, PartialEq, Eq, Hash, Clone, Copy,
)]
#[repr(u8)]
pub enum AgentStatus {
    #[default]
    Offline = 0,
    Online,
    Updating,
}

#[derive(Clone, Debug, Derivative, Serialize, Deserialize)]
#[derivative(Default(new = "true"))]
pub struct AgentCommand {
    pub code: Option<i32>,
    pub output: Vec<u8>,
}

#[derive(Clone, Debug, Derivative, Serialize, Deserialize)]
#[derivative(Default(new = "true"))]
pub struct AgentFile {
    pub code: u32,
    pub message: String,
}

#[derive(Derivative, Serialize, Deserialize, Clone, Debug)]
#[derivative(Default(new = "true"))]
pub struct Agent {
    pub id: HyUuid,
    pub uid: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    pub ip: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    pub last_login: i64,
    pub status: AgentStatus,

    #[serde(skip)]
    pub message: Option<UnboundedSender<Data>>,
    #[serde(skip)]
    pub command: HashMap<HyUuid, Option<AgentCommand>>,
    #[serde(skip)]
    pub file: HashMap<HyUuid, Option<AgentFile>>,

    #[serde(skip_serializing_if = "utils::is_default")]
    pub report_rate: u32,
    #[serde(skip_serializing_if = "utils::is_default")]
    pub disable_shell: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<SocketAddr>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_rsp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f32>, // cpu status, unit percent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>, // memory status, unit bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_memory: Option<u64>, // total memory, unit bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<u64>, // disk status, unit bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_disk: Option<u64>, // total disk, unit bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<i64>, // agent latency, unit ms
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_up: Option<u64>, // network upload, unit bytes/s
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_down: Option<u64>, // network download, unit bytes/s
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band_up: Option<u64>, // bandwidth upload, unit bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band_down: Option<u64>, // bandwidth download, unit bytes
}

impl From<agents::Model> for Agent {
    fn from(v: agents::Model) -> Self {
        Self {
            id: v.id,
            uid: v.uid,
            name: v.name,
            os: v.os,
            hostname: v.hostname,
            ip: v.ip,
            system: v.system,
            arch: v.arch,
            last_login: v.last_login,
            message: None,
            command: HashMap::new(),
            file: HashMap::new(),
            endpoint: String::new(),
            ..Default::default()
        }
    }
}
