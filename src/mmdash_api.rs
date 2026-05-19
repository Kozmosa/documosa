use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use axum::extract::State;

use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::models::BaseRevision;
use crate::mmdash_auth::MmdashIdentity;
use crate::mmdash_blocks::{Block, blocks_to_markdown, markdown_to_blocks};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/mmdash/documents", post(create_document))
        .route("/api/mmdash/documents/{document_id}", get(get_document))
        .route(
            "/api/mmdash/documents/{document_id}/content",
            get(get_content).put(update_content),
        )
}

#[derive(Deserialize)]
struct CreateDocumentRequest {
    title: String,
    #[serde(default)]
    content: String,
}

#[derive(Serialize)]
struct CreateDocumentResponse {
    page_id: String,
    title: String,
    created_at: String,
}

#[derive(Serialize)]
struct DocumentMetadataResponse {
    page_id: String,
    title: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct ContentResponse {
    page_id: String,
    title: String,
    blocks: Vec<Block>,
    markdown: String,
}

#[derive(Deserialize)]
struct UpdateContentRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    markdown: Option<String>,
    #[serde(default)]
    blocks: Option<Vec<Block>>,
}

async fn create_document(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Json(body): Json<CreateDocumentRequest>,
) -> Result<impl IntoResponse> {
    let snapshot = db::create_page(&state.pool, &actor, body.title, body.content).await?;
    let doc = snapshot.page;
    Ok((
        StatusCode::CREATED,
        Json(CreateDocumentResponse {
            page_id: doc.id,
            title: doc.title,
            created_at: doc.created_at,
        }),
    ))
}

async fn get_document(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
) -> Result<impl IntoResponse> {
    let doc = db::snapshot(&state.pool, &document_id).await?.page;
    Ok(Json(DocumentMetadataResponse {
        page_id: doc.id,
        title: doc.title,
        created_at: doc.created_at,
        updated_at: doc.updated_at,
    }))
}

async fn get_content(
    State(_state): State<AppState>,
    _path: Path<String>,
) -> Result<impl IntoResponse> {
    Ok(Json(serde_json::json!({"status": "not_implemented"})))
}

async fn update_content(
    State(_state): State<AppState>,
    _identity: MmdashIdentity,
    _path: Path<String>,
    _body: Json<UpdateContentRequest>,
) -> Result<impl IntoResponse> {
    Ok(Json(serde_json::json!({"status": "not_implemented"})))
}

