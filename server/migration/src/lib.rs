pub use sea_orm_migration::prelude::*;

mod m20260930_000001_create_accounts_and_records;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260930_000001_create_accounts_and_records::Migration)]
    }
}
