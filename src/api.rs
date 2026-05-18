use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::AppState;
use crate::db;
use crate::error::{AppError, Result};
use crate::models::{BaseRevision, HistoryCategory, HistoryListOptions, Identity, RoleMode};
use crate::mmdash_api;
use crate::realtime;

fn identity_from_headers(headers: &HeaderMap) -> Result<Identity> {
    let client_id = header_str(headers, "x-documosa-client-id")?;
    let nickname = header_str(headers, "x-documosa-nickname")?;
    let role_mode = RoleMode::parse(&header_str(headers, "x-documosa-role-mode")?)?;
    if client_id.trim().is_empty() || nickname.trim().is_empty() {
        return Err(AppError::BadRequest(
            "client id and nickname are required".into(),
        ));
    }
    Ok(Identity {
        client_id,
        nickname,
        role_mode,
        actor_kind: Default::default(),
    })
}

fn header_str(headers: &HeaderMap, name: &str) -> Result<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::BadRequest(format!("missing {name} header")))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/documents", get(list_documents).post(create_document))
        .route("/api/documents/{document_id}", get(get_document))
        .route("/api/documents/{document_id}/content", put(update_content))
        .route("/api/documents/{document_id}/history", get(list_history))
        .route(
            "/api/documents/{document_id}/history-diff",
            get(history_diff),
        )
        .route(
            "/api/documents/{document_id}/audit-events/{audit_event_id}/note",
            put(put_audit_event_note),
        )
        .route("/api/documents/{document_id}/export", get(export_document))
        .route(
            "/api/documents/{document_id}/lines/insert",
            post(insert_lines),
        )
        .route(
            "/api/documents/{document_id}/lines/replace",
            post(replace_lines),
        )
        .route(
            "/api/documents/{document_id}/lines/delete",
            post(delete_lines),
        )
        .route(
            "/api/documents/{document_id}/locks/heartbeat",
            post(heartbeat_locks),
        )
        .route(
            "/api/documents/{document_id}/locks/release",
            post(release_locks),
        )
        .route(
            "/api/documents/{document_id}/comments",
            post(create_comment),
        )
        .route(
            "/api/documents/{document_id}/comments/{comment_id}",
            put(update_comment).delete(delete_comment),
        )
        .route(
            "/api/documents/{document_id}/comments/{comment_id}/reply",
            post(reply_comment),
        )
        .route(
            "/api/documents/{document_id}/comments/{comment_id}/resolve",
            post(resolve_comment),
        )
        .route(
            "/api/documents/{document_id}/suggestions",
            post(create_suggestion),
        )
        .route(
            "/api/documents/{document_id}/suggestions/{suggestion_id}/accept",
            post(accept_suggestion),
        )
        .route(
            "/api/documents/{document_id}/suggestions/{suggestion_id}/reject",
            post(reject_suggestion),
        )
        .route("/api/documents/{document_id}/ws", get(ws))
        .merge(mmdash_api::router())
}

#[derive(Deserialize)]
struct CreateDocumentBody {
    title: String,
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct InsertLinesBody {
    after_line_id: Option<String>,
    content: Vec<String>,
}

#[derive(Deserialize)]
struct ReplaceLinesBody {
    line_ids: Vec<String>,
    content: Vec<String>,
}

#[derive(Deserialize)]
struct DeleteLinesBody {
    line_ids: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateContentBody {
    content: String,
    base_revisions: Vec<BaseRevision>,
}

#[derive(Deserialize)]
struct LockBody {
    line_ids: Vec<String>,
}

#[derive(Deserialize)]
struct CommentBody {
    start_line_id: String,
    end_line_id: String,
    start_column: Option<i64>,
    end_column: Option<i64>,
    body: String,
}

#[derive(Deserialize)]
struct UpdateCommentBody {
    body: String,
}

#[derive(Deserialize)]
struct AuditEventNoteBody {
    body: String,
}

#[derive(Deserialize)]
struct HistoryDiffQuery {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct HistoryQuery {
    category: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct ReplyBody {
    body: String,
}

#[derive(Deserialize)]
struct SuggestionBody {
    kind: String,
    anchor_line_id: Option<String>,
    start_line_id: Option<String>,
    end_line_id: Option<String>,
    #[serde(default)]
    content: Vec<String>,
}

#[derive(Deserialize)]
struct WsQuery {
    client_id: String,
    nickname: String,
    role_mode: String,
}

async fn list_documents(State(state): State<AppState>) -> Result<impl IntoResponse> {
    Ok(Json(db::list_documents(&state.pool).await?))
}

async fn create_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateDocumentBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::create_document(&state.pool, &actor, body.title, body.content).await?;
    state
        .hub
        .document_changed(&snapshot.document.id, "document.created");
    Ok(Json(snapshot))
}

async fn get_document(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
) -> Result<impl IntoResponse> {
    Ok(Json(db::snapshot(&state.pool, &document_id).await?))
}

async fn export_document(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
) -> Result<impl IntoResponse> {
    db::export_document(&state.pool, &document_id).await
}

async fn list_history(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse> {
    Ok(Json(
        db::list_history_events(&state.pool, &document_id, history_options(query)?).await?,
    ))
}

async fn history_diff(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
    Query(query): Query<HistoryDiffQuery>,
) -> Result<impl IntoResponse> {
    Ok(Json(
        db::history_diff(&state.pool, &document_id, &query.from, &query.to).await?,
    ))
}

fn history_options(query: HistoryQuery) -> Result<HistoryListOptions> {
    Ok(HistoryListOptions {
        category: HistoryCategory::parse(query.category.as_deref().unwrap_or("document-comment"))?,
        from: query
            .from
            .map(|value| normalize_history_timestamp("from", &value))
            .transpose()?,
        to: query
            .to
            .map(|value| normalize_history_timestamp("to", &value))
            .transpose()?,
        limit: query.limit.unwrap_or(db::HISTORY_DEFAULT_LIMIT),
    })
}

fn normalize_history_timestamp(name: &str, value: &str) -> Result<String> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc).to_rfc3339())
        .map_err(|_| AppError::BadRequest(format!("{name} must be an RFC3339 timestamp")))
}

async fn update_content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
    Json(body): Json<UpdateContentBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::update_content(
        &state.pool,
        &actor,
        &document_id,
        body.content,
        body.base_revisions,
    )
    .await?;
    state
        .hub
        .document_changed(&document_id, "document.content_updated");
    Ok(Json(snapshot))
}

async fn put_audit_event_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((document_id, audit_event_id)): Path<(String, String)>,
    Json(body): Json<AuditEventNoteBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::put_audit_event_note(
        &state.pool,
        &actor,
        &document_id,
        &audit_event_id,
        body.body,
    )
    .await?;
    state
        .hub
        .document_changed(&document_id, "audit.note.updated");
    Ok(Json(snapshot))
}

async fn insert_lines(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
    Json(body): Json<InsertLinesBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::insert_lines(
        &state.pool,
        &actor,
        &document_id,
        body.after_line_id,
        body.content,
    )
    .await?;
    state.hub.document_changed(&document_id, "lines.inserted");
    Ok(Json(snapshot))
}

async fn replace_lines(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
    Json(body): Json<ReplaceLinesBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::replace_lines(
        &state.pool,
        &actor,
        &document_id,
        body.line_ids,
        body.content,
    )
    .await?;
    state.hub.document_changed(&document_id, "lines.replaced");
    Ok(Json(snapshot))
}

async fn delete_lines(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
    Json(body): Json<DeleteLinesBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::delete_lines(&state.pool, &actor, &document_id, body.line_ids).await?;
    state.hub.document_changed(&document_id, "lines.deleted");
    Ok(Json(snapshot))
}

async fn heartbeat_locks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
    Json(body): Json<LockBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let locks = db::heartbeat_locks(&state.pool, &actor, &document_id, body.line_ids).await?;
    state.hub.document_changed(&document_id, "locks.heartbeat");
    Ok(Json(locks))
}

async fn release_locks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
    Json(body): Json<LockBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let locks = db::release_locks(&state.pool, &actor, &document_id, body.line_ids).await?;
    state.hub.document_changed(&document_id, "locks.released");
    Ok(Json(locks))
}

async fn create_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
    Json(body): Json<CommentBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::create_comment(
        &state.pool,
        &actor,
        &document_id,
        db::CommentDraft {
            start_line_id: body.start_line_id,
            end_line_id: body.end_line_id,
            start_column: body.start_column,
            end_column: body.end_column,
            body: body.body,
        },
    )
    .await?;
    state.hub.document_changed(&document_id, "comment.created");
    Ok(Json(snapshot))
}

async fn update_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((document_id, comment_id)): Path<(String, String)>,
    Json(body): Json<UpdateCommentBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot =
        db::update_comment(&state.pool, &actor, &document_id, &comment_id, body.body).await?;
    state.hub.document_changed(&document_id, "comment.updated");
    Ok(Json(snapshot))
}

async fn delete_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((document_id, comment_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::delete_comment(&state.pool, &actor, &document_id, &comment_id).await?;
    state.hub.document_changed(&document_id, "comment.deleted");
    Ok(Json(snapshot))
}

async fn reply_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((document_id, comment_id)): Path<(String, String)>,
    Json(body): Json<ReplyBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot =
        db::reply_comment(&state.pool, &actor, &document_id, &comment_id, body.body).await?;
    state.hub.document_changed(&document_id, "comment.replied");
    Ok(Json(snapshot))
}

async fn resolve_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((document_id, comment_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::resolve_comment(&state.pool, &actor, &document_id, &comment_id).await?;
    state.hub.document_changed(&document_id, "comment.resolved");
    Ok(Json(snapshot))
}

async fn create_suggestion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<String>,
    Json(body): Json<SuggestionBody>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot = db::create_suggestion(
        &state.pool,
        &actor,
        &document_id,
        body.kind,
        body.anchor_line_id,
        body.start_line_id,
        body.end_line_id,
        body.content,
    )
    .await?;
    state
        .hub
        .document_changed(&document_id, "suggestion.created");
    Ok(Json(snapshot))
}

async fn accept_suggestion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((document_id, suggestion_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot =
        db::decide_suggestion(&state.pool, &actor, &document_id, &suggestion_id, true).await?;
    state
        .hub
        .document_changed(&document_id, "suggestion.accepted");
    Ok(Json(snapshot))
}

async fn reject_suggestion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((document_id, suggestion_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let actor = identity_from_headers(&headers)?;
    let snapshot =
        db::decide_suggestion(&state.pool, &actor, &document_id, &suggestion_id, false).await?;
    state
        .hub
        .document_changed(&document_id, "suggestion.rejected");
    Ok(Json(snapshot))
}

async fn ws(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse> {
    let identity = Identity {
        client_id: query.client_id,
        nickname: query.nickname,
        role_mode: RoleMode::parse(&query.role_mode)?,
        actor_kind: Default::default(),
    };
    if identity.client_id.trim().is_empty() || identity.nickname.trim().is_empty() {
        return Err(AppError::BadRequest(
            "client id and nickname are required".into(),
        ));
    }
    Ok(ws.on_upgrade(move |socket| realtime::websocket(socket, state.hub, document_id, identity)))
}
