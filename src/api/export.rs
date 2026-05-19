use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::mmdash_auth::MmdashIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pages/{page_id}/export/md", get(export_markdown))
}

async fn export_markdown(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(page_id): Path<String>,
) -> Result<impl IntoResponse> {
    let markdown = db::export_markdown(&state.pool, &page_id).await?;
    Ok(Json(json!({
        "object": "page",
        "markdown": markdown
    })))
}
