pub use sea_orm_migration::prelude::*;

mod m20260930_000001_create_accounts_and_records;
mod m20261001_000001_create_sessions_and_logins;
mod m20261002_000001_add_account_epoch;
mod m20261007_000001_add_login_claims;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260930_000001_create_accounts_and_records::Migration),
            Box::new(m20261001_000001_create_sessions_and_logins::Migration),
            Box::new(m20261002_000001_add_account_epoch::Migration),
            Box::new(m20261007_000001_add_login_claims::Migration),
        ]
    }
}
