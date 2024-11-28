use skynet_api::{
    anyhow,
    hyuuid::uuids2strings,
    request::Condition,
    sea_orm::{
        self, ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, EntityTrait,
        PaginatorTrait, QueryFilter, Set, Unchanged,
    },
    HyUuid, Result,
};
use skynet_macro::default_viewer;

use crate::entity::passive_agents;

pub struct PassiveAgentViewer;

#[default_viewer(passive_agents)]
impl PassiveAgentViewer {
    /// Create passive agents.
    ///
    /// This method will NOT add agent to server, please invoke `connect` AFTER commit.
    pub async fn create<C>(
        db: &C,
        name: &str,
        address: &str,
        retry_time: i32,
    ) -> Result<passive_agents::Model>
    where
        C: ConnectionTrait,
    {
        passive_agents::ActiveModel {
            name: Set(name.to_owned()),
            address: Set(address.to_owned()),
            retry_time: Set(retry_time),
            ..Default::default()
        }
        .insert(db)
        .await
        .map_err(Into::into)
    }

    pub async fn update<C>(
        db: &C,
        id: &HyUuid,
        name: Option<&str>,
        address: Option<&str>,
        retry_time: Option<i32>,
    ) -> Result<passive_agents::Model>
    where
        C: ConnectionTrait,
    {
        passive_agents::ActiveModel {
            id: Unchanged(id.to_owned()),
            name: name.map_or(NotSet, |x| Set(x.to_owned())),
            address: address.map_or(NotSet, |x| Set(x.to_owned())),
            retry_time: retry_time.map_or(NotSet, |x| Set(x.to_owned())),
            ..Default::default()
        }
        .update(db)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_name<C>(db: &C, name: &str) -> Result<Option<passive_agents::Model>>
    where
        C: ConnectionTrait,
    {
        passive_agents::Entity::find()
            .filter(passive_agents::Column::Name.eq(name))
            .one(db)
            .await
            .map_err(anyhow::Error::from)
    }

    pub async fn find_by_address<C>(db: &C, address: &str) -> Result<Option<passive_agents::Model>>
    where
        C: ConnectionTrait,
    {
        passive_agents::Entity::find()
            .filter(passive_agents::Column::Address.eq(address))
            .one(db)
            .await
            .map_err(anyhow::Error::from)
    }
}
