use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use axum::extract::State;

use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::models::{Identity, PageSnapshot};
use crate::mmdash_auth::MmdashIdentity;
use crate::mmdash_blocks::{Block, BlockType, blocks_to_markdown, markdown_to_blocks};

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
    let mmdash_blocks = markdown_to_blocks(&body.content);
    let blocks_json = mmdash_blocks_to_json(&mmdash_blocks)?;
    let title_rt = json!([{"type":"text","text":{"content":body.title},"plain_text":body.title}]);
    let title_json = serde_json::to_string(&title_rt).unwrap_or_default();
    let snapshot = db::create_page(&state.pool, &actor, title_json, blocks_json).await?;
    let doc = &snapshot.page;
    Ok((
        StatusCode::CREATED,
        Json(CreateDocumentResponse {
            page_id: doc.id.clone(),
            title: doc.plain_title(),
            created_at: doc.created_at.clone(),
        }),
    ))
}

async fn get_document(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
) -> Result<impl IntoResponse> {
    let snap = db::snapshot(&state.pool, &document_id).await?;
    let doc = &snap.page;
    Ok(Json(DocumentMetadataResponse {
        page_id: doc.id.clone(),
        title: doc.plain_title(),
        created_at: doc.created_at.clone(),
        updated_at: doc.updated_at.clone(),
    }))
}

async fn get_content(
    State(state): State<AppState>,
    Path(document_id): Path<String>,
) -> Result<impl IntoResponse> {
    let snap = db::snapshot(&state.pool, &document_id).await?;
    let blocks = blocks_from_snapshot(&snap);
    let markdown = blocks_to_markdown(&blocks);
    Ok(Json(ContentResponse {
        page_id: snap.page.id.clone(),
        title: snap.page.plain_title(),
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
    if let Some(title) = &body.title {
        let title_rt = json!([{"type":"text","text":{"content":title},"plain_text":title}]);
        let title_json = serde_json::to_string(&title_rt).unwrap_or_default();
        db::update_page_title(&state.pool, &actor, &document_id, &title_json).await?;
        state.hub.title_updated(&document_id, title);
    }

    if let Some(blocks) = &body.blocks {
        replace_all_blocks(&state, &actor, &document_id, blocks).await?;
    } else if let Some(markdown) = &body.markdown {
        let blocks = markdown_to_blocks(markdown);
        replace_all_blocks(&state, &actor, &document_id, &blocks).await?;
    }

    let snap = db::snapshot(&state.pool, &document_id).await?;
    let result_blocks = blocks_from_snapshot(&snap);
    let result_markdown = blocks_to_markdown(&result_blocks);
    Ok(Json(ContentResponse {
        page_id: snap.page.id.clone(),
        title: snap.page.plain_title(),
        blocks: result_blocks,
        markdown: result_markdown,
    }))
}

// ── helpers ──

fn mmdash_blocks_block_type(block: &Block) -> &str {
    match block.block_type {
        BlockType::Heading1 => "heading_1",
        BlockType::Heading2 => "heading_2",
        BlockType::Heading3 => "heading_3",
        BlockType::Code => "code",
        BlockType::Equation => "equation",
        BlockType::BulletedListItem => "bulleted_list_item",
        BlockType::NumberedListItem => "numbered_list_item",
        BlockType::Quote => "quote",
        BlockType::Divider => "divider",
        BlockType::Paragraph => "paragraph",
    }
}

fn mmdash_blocks_to_json(blocks: &[Block]) -> Result<String> {
    let inputs: Vec<db::BlockInput> = blocks
        .iter()
        .map(|b| {
            let block_type = mmdash_blocks_block_type(b).to_string();
            let content = b.content.clone().unwrap_or_default();
            let content_json = if matches!(b.block_type, BlockType::Divider) {
                "[]".to_string()
            } else {
                serde_json::to_string(&[serde_json::json!({
                    "type": "text",
                    "text": { "content": content },
                    "plain_text": content,
                })])
                .unwrap_or_else(|_| "[]".to_string())
            };
            let properties_json = match b.block_type {
                BlockType::Code => Some(
                    serde_json::json!({ "language": b.language.clone().unwrap_or_default() })
                        .to_string(),
                ),
                _ => None,
            };
            db::BlockInput {
                block_type,
                content_json,
                properties_json,
            }
        })
        .collect();
    Ok(serde_json::to_string(&inputs)?)
}

fn blocks_from_snapshot(snap: &PageSnapshot) -> Vec<Block> {
    snap.blocks
        .iter()
        .map(|b| {
            let block_type = match b.block_type.as_str() {
                "heading_1" => BlockType::Heading1,
                "heading_2" => BlockType::Heading2,
                "heading_3" => BlockType::Heading3,
                "code" => BlockType::Code,
                "equation" => BlockType::Equation,
                "bulleted_list_item" => BlockType::BulletedListItem,
                "numbered_list_item" => BlockType::NumberedListItem,
                "quote" => BlockType::Quote,
                "divider" => BlockType::Divider,
                _ => BlockType::Paragraph,
            };
            let tokens: Vec<serde_json::Value> =
                serde_json::from_str(&b.content_json).unwrap_or_default();
            let plain_text: String = tokens
                .iter()
                .filter_map(|t| t.get("plain_text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("");
            let language = if matches!(block_type, BlockType::Code) {
                serde_json::from_str::<serde_json::Value>(&b.properties_json)
                    .ok()
                    .and_then(|p| {
                        p.get("language")
                            .and_then(|v| v.as_str().map(str::to_string))
                    })
            } else {
                None
            };
            Block {
                block_type,
                content: Some(plain_text),
                language,
            }
        })
        .collect()
}

async fn replace_all_blocks(
    state: &AppState,
    actor: &Identity,
    page_id: &str,
    blocks: &[Block],
) -> Result<()> {
    sqlx::query(
        "UPDATE blocks SET deleted = 1, revision = revision + 1, updated_at = ? WHERE page_id = ? AND deleted = 0",
    )
    .bind(documosa_core::models::now())
    .bind(page_id)
    .execute(&state.pool)
    .await?;

    let blocks_json = mmdash_blocks_to_json(blocks)?;
    let inputs: Vec<db::BlockInput> = serde_json::from_str(&blocks_json)?;

    let mut tx = db::begin_write_tx(&state.pool).await?;
    for (index, input) in inputs.iter().enumerate() {
        let order_index = (index as f64 + 1.0) * 1000.0;
        db::insert_block_tx(
            &mut tx,
            page_id,
            None,
            order_index,
            &input.block_type,
            &input.content_json,
            input.properties_json.as_deref().unwrap_or("{}"),
        )
        .await?;
    }
    db::touch_page_tx(&mut tx, page_id).await?;
    db::audit_tx(
        &mut tx,
        page_id,
        actor,
        "page.content_updated",
        serde_json::json!({ "blocks_count": inputs.len() }),
    )
    .await?;
    tx.commit().await?;
    state.hub.content_changed(page_id);
    Ok(())
}
