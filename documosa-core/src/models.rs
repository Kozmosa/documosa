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
pub struct Document {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Line {
    pub id: String,
    pub document_id: String,
    pub order_index: i64,
    pub content: String,
    pub revision: i64,
    pub deleted: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct LineLock {
    pub line_id: String,
    pub document_id: String,
    pub owner_client_id: String,
    pub owner_nickname: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Comment {
    pub id: String,
    pub document_id: String,
    pub start_line_id: String,
    pub end_line_id: String,
    pub start_column: Option<i64>,
    pub end_column: Option<i64>,
    pub author_client_id: String,
    pub author_nickname: String,
    pub role_mode: String,
    pub body: String,
    pub resolved: bool,
    pub created_at: String,
    pub updated_at: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Suggestion {
    pub id: String,
    pub document_id: String,
    pub kind: String,
    pub anchor_line_id: Option<String>,
    pub start_line_id: Option<String>,
    pub end_line_id: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct AuditEvent {
    pub id: String,
    pub document_id: String,
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
                "history category must be document-comment, all, content, comment, suggestion, or system".into(),
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
pub struct DocumentSnapshot {
    pub document: Document,
    pub lines: Vec<Line>,
    pub comments: Vec<Comment>,
    pub replies: Vec<CommentReply>,
    pub suggestions: Vec<Suggestion>,
    pub locks: Vec<LineLock>,
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
    pub line_id: String,
    pub revision: i64,
}
