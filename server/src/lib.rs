//! # vsesvit-sync-server
//!
//! Stores Vsesvit's sync records for people signed in with an OpenID Connect provider. The API is
//! `vsesvit_sync_proto`; the storage is SQLite or Postgres through SeaORM, picked by the scheme of
//! `DATABASE_URL`.

pub mod api;
pub mod auth;
pub mod config;
pub mod entities;
pub mod store;

use migration::{Migrator, MigratorTrait};
use config::DatabaseConfig;
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, DbErr};

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error(transparent)]
    Db(#[from] DbErr),
    #[error("the database needs migrations {0}; run them with the migration CLI, or set RUN_MIGRATIONS=true")]
    Pending(String),
}

/// Connects, then applies the migrations this build has and the database lacks. With
/// `run_migrations` off, pending migrations are an error instead: the server would otherwise
/// fail on its first query against the old schema.
pub async fn connect(config: &DatabaseConfig) -> Result<DatabaseConnection, ConnectError> {
    let url = config.url.as_str();
    let mut options = ConnectOptions::new(url);
    options.sqlx_logging(false).min_connections(0).max_connections(config.max_connections);
    let in_memory = url.contains(":memory:") || url.contains("mode=memory");
    if in_memory {
        // Every connection to an in-memory SQLite database opens a database of its own.
        options.max_connections(1);
    }
    let db = Database::connect(options).await?;
    if db.get_database_backend() == DbBackend::Sqlite && !in_memory {
        db.execute_unprepared("PRAGMA journal_mode = WAL").await?;
    }
    let pending: Vec<String> = Migrator::get_pending_migrations(&db).await?.iter().map(|m| m.name().to_owned()).collect();
    if pending.is_empty() {
        return Ok(db);
    }
    if !config.run_migrations {
        return Err(ConnectError::Pending(pending.join(", ")));
    }
    tracing::info!(migrations = %pending.join(", "), "applying migrations");
    Migrator::up(&db, None).await?;
    Ok(db)
}
