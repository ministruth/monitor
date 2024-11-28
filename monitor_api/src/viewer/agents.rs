use skynet_api::{
    anyhow,
    hyuuid::uuids2strings,
    request::Condition,
    sea_orm::{
        self, ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
        QueryFilter, Set, Unchanged,
    },
    HyUuid, Result,
};
use skynet_macro::default_viewer;

use crate::{entity::agents, InfoMessage};

pub struct AgentViewer;

#[default_viewer(agents)]
impl AgentViewer {
    pub async fn find_by_name<C>(db: &C, name: &str) -> Result<Option<agents::Model>>
    where
        C: ConnectionTrait,
    {
        agents::Entity::find()
            .filter(agents::Column::Name.eq(name))
            .one(db)
            .await
            .map_err(anyhow::Error::from)
    }

    pub async fn find_by_uid<C>(db: &C, uid: &str) -> Result<Option<agents::Model>>
    where
        C: ConnectionTrait,
    {
        agents::Entity::find()
            .filter(agents::Column::Uid.eq(uid))
            .one(db)
            .await
            .map_err(anyhow::Error::from)
    }

    /// Update agent `id` with infos.
    pub async fn update<C>(db: &C, id: &HyUuid, data: &InfoMessage) -> Result<agents::Model>
    where
        C: ConnectionTrait,
    {
        agents::ActiveModel {
            id: Unchanged(*id),
            os: Set(data.os.to_owned()),
            system: Set(data.system.to_owned()),
            arch: Set(data.arch.to_owned()),
            hostname: Set(data.hostname.to_owned()),
            ..Default::default()
        }
        .update(db)
        .await
        .map_err(anyhow::Error::from)
    }

    pub async fn rename<C>(db: &C, id: &HyUuid, name: &str) -> Result<agents::Model>
    where
        C: ConnectionTrait,
    {
        agents::ActiveModel {
            id: Unchanged(*id),
            name: Set(name.to_owned()),
            ..Default::default()
        }
        .update(db)
        .await
        .map_err(anyhow::Error::from)
    }
}
