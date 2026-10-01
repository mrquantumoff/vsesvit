use sea_orm::entity::prelude::*;

/// One person at one OpenID Connect provider. `seq` is the last sequence number given to one of
/// their record writes; it never goes back, not even when their records are deleted, so a cursor
/// past it shows the database went back to an older copy, and `epoch` is raised then.
/// `stored_bytes` and `record_count` are what counts against their quota.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "accounts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub issuer: String,
    pub subject: String,
    pub seq: i64,
    pub stored_bytes: i64,
    pub record_count: i64,
    pub created_at: DateTimeUtc,
    pub epoch: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
