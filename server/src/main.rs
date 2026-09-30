use std::io::IsTerminal;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;
use vsesvit_sync_server::api::{self, AppState};
use vsesvit_sync_server::config::Config;

#[tokio::main]
async fn main() -> ExitCode {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_ansi(std::io::stdout().is_terminal())
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env(|name| std::env::var(name).ok())?;
    let db = vsesvit_sync_server::connect(&config.database_url, config.run_migrations).await?;
    tracing::info!(backend = ?db.get_database_backend(), issuer = %config.auth.issuer, "database ready");
    let app = api::router(AppState::new(db, &config));
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}
