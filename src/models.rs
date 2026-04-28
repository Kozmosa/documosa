use axum::http::HeaderMap;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoleMode {
    Reviewer,
    Writer,
}

impl RoleMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RoleMode::Reviewer => "reviewer",
            RoleMode::Writer => "writer",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "reviewer" => Ok(RoleMode::Reviewer),
            "writer" => Ok(RoleMode::Writer),
            _ => Err(AppError::BadRequest(
                "role mode must be reviewer or writer".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub client_id: String,
    pub nickname: String,
    pub role_mode: RoleMode,
}

impl Identity {
    pub fn from_headers(headers: &HeaderMap) -> Result<Self> {
        let client_id = header(headers, "x-documosa-client-id")?;
        let nickname = header(headers, "x-documosa-nickname")?;
        let role_mode = RoleMode::parse(&header(headers, "x-documosa-role-mode")?)?;
        if client_id.trim().is_empty() || nickname.trim().is_empty() {
            return Err(AppError::BadRequest(
                "client id and nickname are required".into(),
            ));
        }
        Ok(Self {
            client_id,
            nickname,
            role_mode,
        })
    }
}

fn header(headers: &HeaderMap, name: &str) -> Result<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::BadRequest(format!("missing {name} header")))
}

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LineLock {
    pub line_id: String,
    pub document_id: String,
    pub owner_client_id: String,
    pub owner_nickname: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommentReply {
    pub id: String,
    pub comment_id: String,
    pub author_client_id: String,
    pub author_nickname: String,
    pub role_mode: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "document-comment" => Ok(Self::DocumentComment),
            "all" => Ok(Self::All),
            "content" => Ok(Self::Content),
            "comment" => Ok(Self::Comment),
            "suggestion" => Ok(Self::Suggestion),
            "system" => Ok(Self::System),
            _ => Err(AppError::BadRequest(
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
