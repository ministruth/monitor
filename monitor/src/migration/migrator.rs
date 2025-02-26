use crate::{ID, migration::m20230101_000001_create_table};
use actix_cloud::async_trait;
use sea_orm_migration::{MigrationTrait, MigratorTrait};
use skynet_api::sea_orm::{
    DynIden,
    sea_query::{Alias, IntoIden, types},
};

pub struct Migrator;

#[async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20230101_000001_create_table::Migration)]
    }

    fn migration_table_name() -> DynIden {
        Alias::new(format!("seaql_migrations_{ID}")).into_iden()
    }
}

pub fn table_prefix(table: &impl types::Iden) -> Alias {
    Alias::new(format!("{}_{}", ID, table.to_string()))
}
