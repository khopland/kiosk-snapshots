mod capture;
mod config;
mod db;
mod error;
mod http;
mod models;
mod scheduler;
mod state;

use std::net::SocketAddr;

use anyhow::Context;
use tokio::net::TcpListener;
use tracing::info;

use crate::capture::CaptureService;
use crate::config::AppConfig;
use crate::db::Database;
use crate::state::{AppState, BrowserHealth};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "screenshot_pi=info,tower_http=info,axum=info".into()),
        )
        .init();

    let config = AppConfig::from_env().context("load config")?;
    config
        .ensure_dirs()
        .await
        .context("prepare app directories")?;

    let db = Database::connect(&config)
        .await
        .context("connect database")?;
    let browser_health = BrowserHealth::default();
    let capture_service = CaptureService::new(config.clone(), browser_health.clone());

    let state = AppState::new(config.clone(), db.clone(), capture_service, browser_health);

    scheduler::spawn_workers(state.clone());

    let app = http::router(state);
    let listener = bind_listener(config.bind_addr).await?;

    info!("listening on {}", config.bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn bind_listener(addr: SocketAddr) -> anyhow::Result<TcpListener> {
    TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))
}
