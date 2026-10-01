use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Accounts {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    TokenHash,
    AccountId,
    CreatedAt,
    LastUsedAt,
}

#[derive(DeriveIden)]
enum Logins {
    Table,
    Id,
    ClientRedirect,
    ClientState,
    ClientChallenge,
    UpstreamVerifier,
    CodeHash,
    AccountId,
    Name,
    CreatedAt,
    AuthorizedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Sessions::Table)
                    // SHA-256 of the bearer token: a copy of the database hands out no sessions.
                    .col(ColumnDef::new(Sessions::TokenHash).binary().not_null().primary_key())
                    .col(ColumnDef::new(Sessions::AccountId).big_integer().not_null())
                    .col(ColumnDef::new(Sessions::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Sessions::LastUsedAt).timestamp_with_time_zone().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Sessions::Table, Sessions::AccountId)
                            .to(Accounts::Table, Accounts::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(Index::create().name("sessions_account").table(Sessions::Table).col(Sessions::AccountId).to_owned())
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Logins::Table)
                    .col(ColumnDef::new(Logins::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Logins::ClientRedirect).text().not_null())
                    .col(ColumnDef::new(Logins::ClientState).text().not_null())
                    .col(ColumnDef::new(Logins::ClientChallenge).text().not_null())
                    .col(ColumnDef::new(Logins::UpstreamVerifier).text().not_null())
                    .col(ColumnDef::new(Logins::CodeHash).binary().null().unique_key())
                    .col(ColumnDef::new(Logins::AccountId).big_integer().null())
                    .col(ColumnDef::new(Logins::Name).text().null())
                    .col(ColumnDef::new(Logins::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Logins::AuthorizedAt).timestamp_with_time_zone().null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Logins::Table, Logins::AccountId)
                            .to(Accounts::Table, Accounts::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(Index::create().name("logins_created").table(Logins::Table).col(Logins::CreatedAt).to_owned())
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Logins::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Sessions::Table).to_owned()).await
    }
}
