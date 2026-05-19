use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::api::wrap_object;
use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::models::{Comment, CommentReply};
use crate::mmdash_auth::MmdashIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/blocks/{block_id}/comments", post(create_comment).get(list_comments))
        .route("/blocks/{block_id}/comments/{comment_id}", patch(update_comment).delete(delete_comment))
        .route("/blocks/{block_id}/comments/{comment_id}/replies", post(reply_comment))
        .route("/blocks/{block_id}/comments/{comment_id}/resolve", post(resolve_comment))
}

async fn resolve_page_from_block(pool: &SqlitePool, block_id: &str) -> Result<String> {
    let block = db::get_block(pool, block_id).await?;
    Ok(block.page_id)
}

#[derive(Deserialize)]
struct CreateCommentBody {
    body: String,
    start_column: Option<i64>,
    end_column: Option<i64>,
}

async fn create_comment(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path(block_id): Path<String>,
    Json(body): Json<CreateCommentBody>,
) -> Result<impl IntoResponse> {
    let page_id = resolve_page_from_block(&state.pool, &block_id).await?;
    let snap = db::create_comment(
        &state.pool,
        &actor,
        &page_id,
        db::CommentDraft {
            target_block_id: block_id.clone(),
            start_column: body.start_column,
            end_column: body.end_column,
            body: body.body,
        },
    )
    .await?;
    state.hub.comment_created(&page_id, snap.comments.last().unwrap().id.as_str());
    Ok(Json(wrap_object("page", snap)))
}

#[derive(Serialize)]
struct BlockCommentsResponse {
    object: &'static str,
    comments: Vec<Comment>,
    replies: Vec<CommentReply>,
}

async fn list_comments(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(block_id): Path<String>,
) -> Result<impl IntoResponse> {
    let _block = db::get_block(&state.pool, &block_id).await?;

    let mut comments = sqlx::query_as::<_, Comment>(
        "SELECT id, page_id, target_block_id, start_column, end_column, author_client_id, author_nickname, role_mode, body, resolved, created_at, updated_at FROM comments WHERE target_block_id = ? ORDER BY created_at",
    )
    .bind(&block_id)
    .fetch_all(&state.pool)
    .await?;
    for comment in &mut comments { comment.object = "comment".into(); }

    let comment_ids: Vec<String> = comments.iter().map(|c| c.id.clone()).collect();
    let mut replies = if comment_ids.is_empty() {
        vec![]
    } else {
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT id, comment_id, author_client_id, author_nickname, role_mode, body, created_at FROM comment_replies WHERE comment_id IN (",
        );
        let mut separated = qb.separated(", ");
        for id in &comment_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(") ORDER BY created_at");
        qb.build_query_as::<CommentReply>()
            .fetch_all(&state.pool)
            .await?
    };
    for reply in &mut replies { reply.object = "comment_reply".into(); }
    Ok(Json(BlockCommentsResponse {
        object: "list",
        comments,
        replies,
    }))
}

#[derive(Deserialize)]
struct UpdateCommentBody {
    body: String,
}

async fn update_comment(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path((block_id, comment_id)): Path<(String, String)>,
    Json(body): Json<UpdateCommentBody>,
) -> Result<impl IntoResponse> {
    let page_id = resolve_page_from_block(&state.pool, &block_id).await?;
    let snap = db::update_comment(&state.pool, &actor, &page_id, &comment_id, body.body).await?;
    state.hub.content_changed(&page_id);
    Ok(Json(wrap_object("page", snap)))
}

async fn delete_comment(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path((block_id, comment_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let page_id = resolve_page_from_block(&state.pool, &block_id).await?;
    let snap = db::delete_comment(&state.pool, &actor, &page_id, &comment_id).await?;
    state.hub.content_changed(&page_id);
    Ok(Json(wrap_object("page", snap)))
}

#[derive(Deserialize)]
struct ReplyBody {
    body: String,
}

async fn reply_comment(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path((block_id, comment_id)): Path<(String, String)>,
    Json(body): Json<ReplyBody>,
) -> Result<impl IntoResponse> {
    let page_id = resolve_page_from_block(&state.pool, &block_id).await?;
    let snap = db::reply_comment(&state.pool, &actor, &page_id, &comment_id, body.body).await?;
    state.hub.content_changed(&page_id);
    Ok(Json(wrap_object("page", snap)))
}

async fn resolve_comment(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path((block_id, comment_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    let page_id = resolve_page_from_block(&state.pool, &block_id).await?;
    let snap = db::resolve_comment(&state.pool, &actor, &page_id, &comment_id).await?;
    state.hub.comment_resolved(&page_id, &comment_id);
    Ok(Json(wrap_object("page", snap)))
}
