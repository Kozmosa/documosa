use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{get, patch};
use axum::{Json, Router};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};

use crate::api::wrap_object;
use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::models::Block;
use crate::mmdash_auth::MmdashIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/blocks/{block_id}", get(get_block).patch(update_block).delete(delete_block))
        .route("/blocks/{block_id}/children", get(list_children))
        .route("/pages/{page_id}/children", patch(append_blocks))
}

async fn get_block(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(block_id): Path<String>,
) -> Result<impl IntoResponse> {
    let block = db::get_block(&state.pool, &block_id).await?;
    Ok(Json(wrap_object("block", block)))
}

#[derive(Deserialize)]
struct ListChildrenQuery {
    page_size: Option<i64>,
    start_cursor: Option<String>,
}

#[derive(Serialize)]
struct ListChildrenResponse {
    object: &'static str,
    results: Vec<Block>,
    next_cursor: Option<String>,
    has_more: bool,
}

async fn list_children(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(block_id): Path<String>,
    Query(query): Query<ListChildrenQuery>,
) -> Result<impl IntoResponse> {
    let block = db::get_block(&state.pool, &block_id).await?;
    let page_size = query.page_size.unwrap_or(50).max(1).min(100);
    let cursor: Option<f64> = query
        .start_cursor
        .and_then(|s| BASE64.decode(s).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|s| s.parse().ok());
    let (blocks, next_cursor, has_more) = db::list_children(
        &state.pool,
        Some(&block_id),
        &block.page_id,
        cursor,
        page_size,
    )
    .await?;
    let encoded_cursor = next_cursor.map(|order| BASE64.encode(order.to_string()));
    Ok(Json(ListChildrenResponse {
        object: "list",
        results: blocks,
        next_cursor: encoded_cursor,
        has_more,
    }))
}

#[derive(Deserialize)]
struct UpdateBlockRequest {
    block_type: Option<String>,
    content_json: Option<String>,
    properties_json: Option<String>,
}

async fn update_block(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path(block_id): Path<String>,
    Json(body): Json<UpdateBlockRequest>,
) -> Result<impl IntoResponse> {
    let snap = db::update_block(
        &state.pool,
        &actor,
        &block_id,
        body.block_type.as_deref(),
        body.content_json.as_deref(),
        body.properties_json.as_deref(),
    )
    .await?;
    state.hub.block_updated(&snap.page.id, &block_id);
    Ok(Json(wrap_object("page", snap)))
}

async fn delete_block(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path(block_id): Path<String>,
) -> Result<impl IntoResponse> {
    let snap = db::delete_block(&state.pool, &actor, &block_id).await?;
    state.hub.block_deleted(&snap.page.id, &[block_id]);
    Ok(Json(wrap_object("page", snap)))
}

#[derive(Deserialize)]
struct AppendBlocksBody {
    children: Vec<AppendBlockItem>,
    after: Option<String>,
}

#[derive(Deserialize)]
struct AppendBlockItem {
    block_type: String,
    #[serde(default)]
    content_json: String,
    #[serde(default)]
    properties_json: Option<String>,
}

async fn append_blocks(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path(page_id): Path<String>,
    Json(body): Json<AppendBlocksBody>,
) -> Result<impl IntoResponse> {
    let block_inputs: Vec<db::BlockInput> = body
        .children
        .into_iter()
        .map(|c| db::BlockInput {
            block_type: c.block_type,
            content_json: c.content_json,
            properties_json: c.properties_json,
        })
        .collect();
    let snap = db::append_blocks(&state.pool, &actor, &page_id, block_inputs, body.after.as_deref()).await?;
    state.hub.block_inserted(&page_id, &[], body.after.as_deref());
    Ok(Json(wrap_object("page", snap)))
}
