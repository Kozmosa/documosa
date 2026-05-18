use serde::{Deserialize, Serialize};

use crate::identity::Identity;
use crate::models::*;

/// Protocol-level operation signatures.
/// Each method describes what the system can do.
/// Implementations (e.g. SQLite-backed server) fulfill this trait.
#[allow(async_fn_in_trait)]
pub trait DocumosaOps {
    // ── document ──

    async fn create_document(
        &self,
        actor: &Identity,
        title: String,
        content: String,
    ) -> Result<DocumentSnapshot, crate::error::ProtocolError>;

    async fn list_documents(&self) -> Result<Vec<Document>, crate::error::ProtocolError>;

    async fn get_document(
        &self,
        document_id: &str,
    ) -> Result<DocumentSnapshot, crate::error::ProtocolError>;

    async fn export_document(&self, document_id: &str)
        -> Result<String, crate::error::ProtocolError>;

    async fn update_title(
        &self,
        actor: &Identity,
        document_id: &str,
        title: &str,
    ) -> Result<(), crate::error::ProtocolError>;

    // ── lines ──

    async fn insert_lines(
        &self,
        actor: &Identity,
        document_id: &str,
        anchor_line_id: Option<&str>,
        lines: Vec<String>,
    ) -> Result<Vec<Line>, crate::error::ProtocolError>;

    async fn replace_lines(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
        new_contents: Vec<String>,
        base_revisions: Vec<BaseRevision>,
    ) -> Result<Vec<Line>, crate::error::ProtocolError>;

    async fn delete_lines(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
    ) -> Result<Vec<Line>, crate::error::ProtocolError>;

    // ── locks ──

    async fn lock_lines(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
        ttl_seconds: i64,
    ) -> Result<Vec<LineLock>, crate::error::ProtocolError>;

    async fn heartbeat_locks(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
    ) -> Result<Vec<LineLock>, crate::error::ProtocolError>;

    async fn release_locks(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
    ) -> Result<(), crate::error::ProtocolError>;

    // ── comments ──

    async fn create_comment(
        &self,
        actor: &Identity,
        document_id: &str,
        start_line_id: &str,
        end_line_id: &str,
        start_column: Option<i64>,
        end_column: Option<i64>,
        body: &str,
    ) -> Result<Comment, crate::error::ProtocolError>;

    async fn reply_comment(
        &self,
        actor: &Identity,
        document_id: &str,
        comment_id: &str,
        body: &str,
    ) -> Result<CommentReply, crate::error::ProtocolError>;

    async fn update_comment(
        &self,
        actor: &Identity,
        document_id: &str,
        comment_id: &str,
        body: &str,
    ) -> Result<Comment, crate::error::ProtocolError>;

    async fn resolve_comment(
        &self,
        actor: &Identity,
        document_id: &str,
        comment_id: &str,
    ) -> Result<Comment, crate::error::ProtocolError>;

    // ── suggestions ──

    async fn create_suggestion(
        &self,
        actor: &Identity,
        document_id: &str,
        kind: SuggestionKind,
        anchor_line_id: Option<&str>,
        start_line_id: Option<&str>,
        end_line_id: Option<&str>,
        new_content: &str,
        base_revisions: Vec<BaseRevision>,
    ) -> Result<Suggestion, crate::error::ProtocolError>;

    async fn accept_suggestion(
        &self,
        actor: &Identity,
        document_id: &str,
        suggestion_id: &str,
    ) -> Result<Suggestion, crate::error::ProtocolError>;

    async fn reject_suggestion(
        &self,
        actor: &Identity,
        document_id: &str,
        suggestion_id: &str,
    ) -> Result<Suggestion, crate::error::ProtocolError>;

    // ── history ──

    async fn list_history(
        &self,
        document_id: &str,
        category: HistoryCategory,
        from: Option<&str>,
        to: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AuditEvent>, crate::error::ProtocolError>;

    async fn history_diff(
        &self,
        document_id: &str,
        from_event_id: &str,
        to_event_id: &str,
    ) -> Result<HistoryDiff, crate::error::ProtocolError>;

    async fn set_audit_note(
        &self,
        actor: &Identity,
        document_id: &str,
        audit_event_id: &str,
        body: &str,
    ) -> Result<(), crate::error::ProtocolError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    InsertLines,
    ReplaceLines,
    DeleteLines,
}
