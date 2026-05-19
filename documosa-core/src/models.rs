use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Page {
    pub id: String,
    pub title_json: String,
    pub properties_json: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Page {
    pub fn plain_title(&self) -> String {
        let tokens: Vec<serde_json::Value> =
            serde_json::from_str(&self.title_json).unwrap_or_default();
        tokens
            .iter()
            .filter_map(|t| t.get("plain_text").and_then(|v| v.as_str()))
            .collect()
    }

    pub fn plain_title_from_json(title_json: &str) -> String {
        let tokens: Vec<serde_json::Value> =
            serde_json::from_str(title_json).unwrap_or_default();
        tokens
            .iter()
            .filter_map(|t| t.get("plain_text").and_then(|v| v.as_str()))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Block {
    pub id: String,
    pub page_id: String,
    pub parent_id: Option<String>,
    pub order_index: f64,
    pub block_type: String,
    pub content_json: String,
    pub properties_json: String,
    pub revision: i64,
    pub deleted: bool,
    pub created_at: String,
    pub updated_at: String,
    #[cfg_attr(feature = "sqlx", sqlx(default))]
    #[serde(default = "default_block_object")]
    pub object: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct BlockLock {
    pub block_id: String,
    pub page_id: String,
    pub owner_client_id: String,
    pub owner_nickname: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Comment {
    pub id: String,
    pub page_id: String,
    pub target_block_id: String,
    pub start_column: Option<i64>,
    pub end_column: Option<i64>,
    pub author_client_id: String,
    pub author_nickname: String,
    pub role_mode: String,
    pub body: String,
    pub resolved: bool,
    pub created_at: String,
    pub updated_at: String,
    #[cfg_attr(feature = "sqlx", sqlx(default))]
    #[serde(default = "default_comment_object")]
    pub object: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct CommentReply {
    pub id: String,
    pub comment_id: String,
    pub author_client_id: String,
    pub author_nickname: String,
    pub role_mode: String,
    pub body: String,
    pub created_at: String,
    #[cfg_attr(feature = "sqlx", sqlx(default))]
    #[serde(default = "default_comment_reply_object")]
    pub object: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Suggestion {
    pub id: String,
    pub page_id: String,
    pub kind: String,
    pub target_block_id: Option<String>,
    pub parent_id: Option<String>,
    pub content_json: String,
    pub base_revisions_json: String,
    pub state: String,
    pub author_client_id: String,
    pub author_nickname: String,
    pub role_mode: String,
    pub created_at: String,
    pub decided_by_client_id: Option<String>,
    pub decided_by_nickname: Option<String>,
    pub decided_at: Option<String>,
    #[cfg_attr(feature = "sqlx", sqlx(default))]
    #[serde(default = "default_suggestion_object")]
    pub object: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct AuditEvent {
    pub id: String,
    pub page_id: String,
    pub actor_client_id: String,
    pub actor_nickname: String,
    pub role_mode: String,
    pub event_type: String,
    pub details_json: String,
    pub created_at: String,
    pub note_body: Option<String>,
    pub note_updated_by_nickname: Option<String>,
    pub note_updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HistoryCategory {
    DocumentComment,
    All,
    Content,
    Comment,
    Suggestion,
    System,
}

impl HistoryCategory {
    pub fn parse(value: &str) -> Result<Self, crate::error::ProtocolError> {
        match value {
            "document-comment" => Ok(Self::DocumentComment),
            "all" => Ok(Self::All),
            "content" => Ok(Self::Content),
            "comment" => Ok(Self::Comment),
            "suggestion" => Ok(Self::Suggestion),
            "system" => Ok(Self::System),
            _ => Err(crate::error::ProtocolError::BadRequest(
                "history category must be document-comment, all, content, comment, suggestion, or system"
                    .into(),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HistoryListOptions {
    pub category: HistoryCategory,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSnapshot {
    pub page: Page,
    pub blocks: Vec<Block>,
    pub comments: Vec<Comment>,
    pub replies: Vec<CommentReply>,
    pub suggestions: Vec<Suggestion>,
    pub locks: Vec<BlockLock>,
    pub audit_events: Vec<AuditEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryDiff {
    pub from_event: AuditEvent,
    pub to_event: AuditEvent,
    pub from_content: String,
    pub to_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseRevision {
    pub block_id: String,
    pub revision: i64,
}

fn default_block_object() -> String { "block".into() }
fn default_comment_object() -> String { "comment".into() }
fn default_comment_reply_object() -> String { "comment_reply".into() }
fn default_suggestion_object() -> String { "suggestion".into() }
