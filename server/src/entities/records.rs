use sea_orm::entity::prelude::*;

/// The last body a client uploaded for one record. The server never reads `body`.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "records")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub account_id: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub kind: i16,
    /// SHA-256 of `record_id`, which can be too long to index.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id_hash: Vec<u8>,
    pub record_id: String,
    pub body: Vec<u8>,
    pub size: i64,
    pub seq: i64,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
