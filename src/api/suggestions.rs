use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::api::wrap_object;
use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::mmdash_auth::MmdashIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pages/{page_id}/suggestions", post(create_suggestion))
        .route("/pages/{page_id}/suggestions/{suggestion_id}/accept", post(accept_suggestion))
        .route("/pages/{page_id}/suggestions/{suggestion_id}/reject", post(reject_suggestion))
}

#[derive(Deserialize)]
struct CreateSuggestionBody {
    kind: String,
    target_block_id: Option<String>,
    #[serde(default)]
    content: Vec<String>,
}

async fn create_suggestion(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path(page_id): Path<String>,
    Json(body): Json<CreateSuggestionBody>,
) -> Result<impl IntoResponse> {
    let snap = db::create_suggestion(
        &state.pool,
        &actor,
        &page_id,
        body.kind,
        body.target_block_id,
        body.content,
    )
    .await?;
    state.hub.suggestion_created(&page_id, snap.suggestions.last().unwrap().id.as_str());
    Ok(Json(wrap_object("page", snap)))
}

async fn accept_suggestion(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path((page_id, suggestion_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let snap = db::decide_suggestion(&state.pool, &actor, &page_id, &suggestion_id, true).await?;
    state.hub.suggestion_decided(&page_id, &suggestion_id, true);
    Ok(Json(wrap_object("page", snap)))
}

async fn reject_suggestion(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path((page_id, suggestion_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let snap = db::decide_suggestion(&state.pool, &actor, &page_id, &suggestion_id, false).await?;
    state.hub.suggestion_decided(&page_id, &suggestion_id, false);
    Ok(Json(wrap_object("page", snap)))
}
