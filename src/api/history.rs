use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::AppState;
use crate::db;
use crate::error::{AppError, Result};
use crate::models::{HistoryCategory, HistoryListOptions};
use crate::mmdash_auth::MmdashIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pages/{page_id}/history", axum::routing::get(list_history))
        .route("/pages/{page_id}/history-diff", axum::routing::get(history_diff))
        .route("/pages/{page_id}/audit/{event_id}/note", post(set_note))
}

#[derive(Deserialize)]
struct HistoryQuery {
    category: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<i64>,
}

async fn list_history(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(page_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse> {
    let options = HistoryListOptions {
        category: HistoryCategory::parse(query.category.as_deref().unwrap_or("document-comment"))?,
        from: query
            .from
            .map(|v| normalize_timestamp("from", &v))
            .transpose()?,
        to: query
            .to
            .map(|v| normalize_timestamp("to", &v))
            .transpose()?,
        limit: query.limit.unwrap_or(db::HISTORY_DEFAULT_LIMIT),
    };
    Ok(Json(
        db::list_history_events(&state.pool, &page_id, options).await?,
    ))
}

#[derive(Deserialize)]
struct HistoryDiffQuery {
    from: String,
    to: String,
}

async fn history_diff(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(page_id): Path<String>,
    Query(query): Query<HistoryDiffQuery>,
) -> Result<impl IntoResponse> {
    Ok(Json(
        db::history_diff(&state.pool, &page_id, &query.from, &query.to).await?,
    ))
}

#[derive(Deserialize)]
struct SetNoteBody {
    body: String,
}

async fn set_note(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path((page_id, event_id)): Path<(String, String)>,
    Json(body): Json<SetNoteBody>,
) -> Result<impl IntoResponse> {
    let snap = db::put_audit_event_note(&state.pool, &actor, &page_id, &event_id, body.body).await?;
    state.hub.content_changed(&page_id);
    Ok(Json(snap))
}

fn normalize_timestamp(name: &str, value: &str) -> Result<String> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
        .map_err(|_| AppError::BadRequest(format!("{name} must be an RFC3339 timestamp")))
}
