use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Logins {
    Table,
    Claims,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let claims = ColumnDef::new(Logins::Claims).text().null().to_owned();
        manager.alter_table(Table::alter().table(Logins::Table).add_column(claims).to_owned()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter().table(Logins::Table).drop_column(Logins::Claims).to_owned()).await
    }
}
