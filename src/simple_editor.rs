use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AppState;
use crate::mmdash_blocks::{Block, BlockType, blocks_to_markdown, markdown_to_blocks};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/simple/editor/md2blocks", post(md2blocks))
        .route("/simple/editor/blocks2md", post(blocks2md))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleEditorBlock {
    #[serde(default)]
    pub block_type: String,
    #[serde(default)]
    pub content_json: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Md2BlocksRequest {
    markdown: String,
}

#[derive(Debug, Serialize)]
struct Md2BlocksResponse {
    blocks: Vec<SimpleEditorBlock>,
}

#[derive(Debug, Deserialize)]
struct Blocks2MdRequest {
    blocks: Vec<SimpleEditorBlock>,
}

#[derive(Debug, Serialize)]
struct Blocks2MdResponse {
    markdown: String,
}

async fn md2blocks(Json(body): Json<Md2BlocksRequest>) -> Json<Md2BlocksResponse> {
    let blocks = markdown_to_blocks(&body.markdown)
        .into_iter()
        .map(SimpleEditorBlock::from_mmdash_block)
        .collect();

    Json(Md2BlocksResponse { blocks })
}

async fn blocks2md(Json(body): Json<Blocks2MdRequest>) -> Json<Blocks2MdResponse> {
    let blocks: Vec<Block> = body
        .blocks
        .iter()
        .map(SimpleEditorBlock::to_mmdash_block)
        .collect();
    let markdown = blocks_to_markdown(&blocks);

    Json(Blocks2MdResponse { markdown })
}

impl SimpleEditorBlock {
    fn from_mmdash_block(block: Block) -> Self {
        let content_json = match block.block_type {
            BlockType::Divider => "[]".to_string(),
            _ => rich_text_json(block.content.as_deref().unwrap_or_default()),
        };
        let properties_json = match block.block_type {
            BlockType::Code => block
                .language
                .as_deref()
                .filter(|language| !language.is_empty())
                .map(|language| json!({ "language": language }).to_string()),
            _ => None,
        };

        Self {
            block_type: block_type_str(&block.block_type).to_string(),
            content_json,
            properties_json,
        }
    }

    fn to_mmdash_block(&self) -> Block {
        let block_type = parse_block_type(&self.block_type);
        let content = if matches!(block_type, BlockType::Divider) {
            None
        } else {
            Some(plain_text_from_content_json(&self.content_json))
        };
        let language = if matches!(block_type, BlockType::Code) {
            self.properties_json
                .as_deref()
                .and_then(|properties| serde_json::from_str::<Value>(properties).ok())
                .and_then(|properties| {
                    properties
                        .get("language")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        } else {
            None
        };

        Block {
            block_type,
            content,
            language,
        }
    }
}

fn block_type_str(block_type: &BlockType) -> &'static str {
    match block_type {
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

fn parse_block_type(block_type: &str) -> BlockType {
    match block_type {
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
    }
}

fn rich_text_json(text: &str) -> String {
    serde_json::to_string(&[json!({
        "type": "text",
        "text": { "content": text },
        "plain_text": text,
    })])
    .unwrap_or_else(|_| "[]".to_string())
}

fn plain_text_from_content_json(content_json: &str) -> String {
    let tokens: Vec<Value> = serde_json::from_str(content_json).unwrap_or_default();
    tokens
        .iter()
        .filter_map(|token| token.get("plain_text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}
