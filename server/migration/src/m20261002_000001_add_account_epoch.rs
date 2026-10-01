use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Accounts {
    Table,
    Epoch,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let epoch = ColumnDef::new(Accounts::Epoch).big_integer().not_null().default(0).to_owned();
        manager.alter_table(Table::alter().table(Accounts::Table).add_column(epoch).to_owned()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter().table(Accounts::Table).drop_column(Accounts::Epoch).to_owned()).await
    }
}
