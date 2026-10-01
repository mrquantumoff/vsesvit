use sea_orm::entity::prelude::*;

/// A sign-in under way: the browser's side of it (its loopback redirect, state and PKCE challenge),
/// the server's side with the provider (its PKCE verifier), and, once the provider has vouched for
/// the person, the account and the hash of the one-time code the browser trades for a session.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "logins")]
pub struct Model {
    /// Also the `state` sent to the provider.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub client_redirect: String,
    pub client_state: String,
    pub client_challenge: String,
    pub upstream_verifier: String,
    pub code_hash: Option<Vec<u8>>,
    pub account_id: Option<i64>,
    pub name: Option<String>,
    pub created_at: DateTimeUtc,
    pub authorized_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
