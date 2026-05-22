pub mod api;
pub mod cli;
pub mod db;
pub mod diff;
pub mod error;
pub mod mcp;
pub mod mmdash_api;
pub mod mmdash_auth;
pub mod mmdash_blocks;
pub mod models;
pub mod notion;
pub mod realtime;
pub mod simple_editor;
pub mod static_files;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use sqlx::SqlitePool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::realtime::EventHub;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub hub: EventHub,
    pub web_dir: Arc<PathBuf>,
}

pub async fn build_app(pool: SqlitePool, web_dir: PathBuf) -> Router {
    let state = AppState {
        pool,
        hub: EventHub::new(),
        web_dir: Arc::new(web_dir),
    };

    api::router()
        .merge(mcp::router())
        .merge(simple_editor::router())
        .fallback(static_files::serve)
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

pub async fn serve(addr: String, port: u16, data_dir: PathBuf) -> anyhow::Result<()> {
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("failed to create data dir {}", data_dir.display()))?;
    let db_path = data_dir.join("documosa.sqlite");
    let pool = db::connect(&db_path).await?;
    db::migrate(&pool).await?;

    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/dist");
    let app = build_app(pool, web_dir).await;
    let listener = bind_available_listener(&addr, port).await?;
    tracing::info!("documosa listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn bind_available_listener(
    addr: &str,
    port: u16,
) -> anyhow::Result<tokio::net::TcpListener> {
    let mut candidate = port;
    loop {
        match tokio::net::TcpListener::bind((addr, candidate)).await {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && candidate < u16::MAX => {
                candidate += 1;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to bind documosa on {addr}:{candidate}"));
            }
        }
    }
}
