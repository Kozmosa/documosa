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
    let snapshot = db::create_document(&state.pool, &actor, body.title, body.content).await?;
    let doc = snapshot.document;
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
    let doc = db::snapshot(&state.pool, &document_id).await?.document;
    Ok(Json(DocumentMetadataResponse {
        page_id: doc.id,
        title: doc.title,
        created_at: doc.created_at,
        updated_at: doc.updated_at,
    }))
}

async fn get_content(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
) -> Result<impl IntoResponse> {
    let snapshot = db::snapshot(&state.pool, &document_id).await?;
    let lines: Vec<String> = snapshot
        .lines
        .into_iter()
        .filter(|line| !line.deleted)
        .map(|line| line.content)
        .collect();
    let markdown = lines.join("\n");
    let blocks = markdown_to_blocks(&markdown);
    let doc = snapshot.document;
    Ok(Json(ContentResponse {
        page_id: doc.id,
        title: doc.title,
        blocks,
        markdown,
    }))
}

async fn update_content(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path(document_id): Path<String>,
    Json(body): Json<UpdateContentRequest>,
) -> Result<impl IntoResponse> {
    // Prefer blocks over markdown
    let markdown = if let Some(blocks) = body.blocks {
        blocks_to_markdown(&blocks)
    } else {
        body.markdown.unwrap_or_default()
    };

    // Build base_revisions from current active lines
    let snapshot = db::snapshot(&state.pool, &document_id).await?;
    let base_revisions: Vec<BaseRevision> = snapshot
        .lines
        .into_iter()
        .filter(|line| !line.deleted)
        .map(|line| BaseRevision {
            line_id: line.id,
            revision: line.revision,
        })
        .collect();

    let updated = db::update_content(
        &state.pool,
        &actor,
        &document_id,
        markdown.clone(),
        base_revisions,
    )
    .await?;

    // Optionally update title (audited)
    let final_title = if let Some(ref title) = body.title {
        db::update_document_title(&state.pool, &actor, &document_id, title).await?;
        title.clone()
    } else {
        updated.document.title
    };

    let lines: Vec<String> = updated
        .lines
        .into_iter()
        .filter(|line| !line.deleted)
        .map(|line| line.content)
        .collect();
    let final_markdown = lines.join("\n");
    let blocks = markdown_to_blocks(&final_markdown);

    Ok(Json(ContentResponse {
        page_id: document_id,
        title: final_title,
        blocks,
        markdown: final_markdown,
    }))
}

