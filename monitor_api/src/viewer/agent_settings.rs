use skynet_api::{
    HyUuid, anyhow,
    hyuuid::uuids2strings,
    request::Condition,
    sea_orm::{self, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter},
};
use skynet_macro::default_viewer;

use crate::entity::agent_settings;

pub struct AgentSettingViewer;

#[default_viewer(agent_settings)]
impl AgentSettingViewer {}
