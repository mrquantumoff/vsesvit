use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Accounts {
    Table,
    Id,
    Issuer,
    Subject,
    Seq,
    StoredBytes,
    RecordCount,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Records {
    Table,
    AccountId,
    Kind,
    IdHash,
    RecordId,
    Body,
    Size,
    Seq,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// SeaORM runs SQLite migrations outside a transaction unless asked, and a crash between two
    /// tables would leave this one half applied and unable to run again.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Accounts::Table)
                    .col(ColumnDef::new(Accounts::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Accounts::Issuer).text().not_null())
                    .col(ColumnDef::new(Accounts::Subject).text().not_null())
                    .col(ColumnDef::new(Accounts::Seq).big_integer().not_null().default(0))
                    .col(ColumnDef::new(Accounts::StoredBytes).big_integer().not_null().default(0))
                    .col(ColumnDef::new(Accounts::RecordCount).big_integer().not_null().default(0))
                    .col(ColumnDef::new(Accounts::CreatedAt).timestamp_with_time_zone().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("accounts_issuer_subject")
                    .table(Accounts::Table)
                    .col(Accounts::Issuer)
                    .col(Accounts::Subject)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Records::Table)
                    .col(ColumnDef::new(Records::AccountId).big_integer().not_null())
                    .col(ColumnDef::new(Records::Kind).small_integer().not_null())
                    // SHA-256 of `record_id`: history ids are URLs, and a long one would pass
                    // Postgres's limit for one B-tree entry.
                    .col(ColumnDef::new(Records::IdHash).binary().not_null())
                    .col(ColumnDef::new(Records::RecordId).text().not_null())
                    .col(ColumnDef::new(Records::Body).blob().not_null())
                    .col(ColumnDef::new(Records::Size).big_integer().not_null())
                    .col(ColumnDef::new(Records::Seq).big_integer().not_null())
                    .col(ColumnDef::new(Records::UpdatedAt).timestamp_with_time_zone().not_null())
                    .primary_key(Index::create().col(Records::AccountId).col(Records::Kind).col(Records::IdHash))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Records::Table, Records::AccountId)
                            .to(Accounts::Table, Accounts::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("records_account_seq")
                    .table(Records::Table)
                    .col(Records::AccountId)
                    .col(Records::Seq)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Records::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Accounts::Table).to_owned()).await
    }
}
