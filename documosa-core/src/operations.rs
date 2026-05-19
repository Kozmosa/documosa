use serde::{Deserialize, Serialize};

use crate::identity::Identity;
use crate::models::*;

/// Protocol-level operation signatures.
/// Each method describes what the system can do.
/// Implementations (e.g. SQLite-backed server) fulfill this trait.
#[allow(async_fn_in_trait)]
pub trait DocumosaOps {
    // ── page ──

    async fn create_page(
        &self,
        actor: &Identity,
        title: String,
        content: String,
    ) -> Result<PageSnapshot, crate::error::ProtocolError>;

    async fn list_pages(&self) -> Result<Vec<Page>, crate::error::ProtocolError>;

    async fn get_page(
        &self,
        page_id: &str,
    ) -> Result<PageSnapshot, crate::error::ProtocolError>;

    async fn export_page(&self, page_id: &str)
        -> Result<String, crate::error::ProtocolError>;

    async fn update_title(
        &self,
        actor: &Identity,
        page_id: &str,
        title: &str,
    ) -> Result<(), crate::error::ProtocolError>;

    // ── blocks ──

    async fn insert_blocks(
        &self,
        actor: &Identity,
        page_id: &str,
        anchor_block_id: Option<&str>,
        blocks: Vec<String>,
    ) -> Result<Vec<Block>, crate::error::ProtocolError>;

    async fn replace_blocks(
        &self,
        actor: &Identity,
        page_id: &str,
        block_ids: Vec<&str>,
        new_contents: Vec<String>,
        base_revisions: Vec<BaseRevision>,
    ) -> Result<Vec<Block>, crate::error::ProtocolError>;

    async fn delete_blocks(
        &self,
        actor: &Identity,
        page_id: &str,
        block_ids: Vec<&str>,
    ) -> Result<Vec<Block>, crate::error::ProtocolError>;

    // ── locks ──

    async fn lock_blocks(
        &self,
        actor: &Identity,
        page_id: &str,
        block_ids: Vec<&str>,
        ttl_seconds: i64,
    ) -> Result<Vec<BlockLock>, crate::error::ProtocolError>;

    async fn heartbeat_locks(
        &self,
        actor: &Identity,
        page_id: &str,
        block_ids: Vec<&str>,
    ) -> Result<Vec<BlockLock>, crate::error::ProtocolError>;

    async fn release_locks(
        &self,
        actor: &Identity,
        page_id: &str,
        block_ids: Vec<&str>,
    ) -> Result<(), crate::error::ProtocolError>;

    // ── comments ──

    async fn create_comment(
        &self,
        actor: &Identity,
        page_id: &str,
        target_block_id: &str,
        start_column: Option<i64>,
        end_column: Option<i64>,
        body: &str,
    ) -> Result<Comment, crate::error::ProtocolError>;

    async fn reply_comment(
        &self,
        actor: &Identity,
        page_id: &str,
        comment_id: &str,
        body: &str,
    ) -> Result<CommentReply, crate::error::ProtocolError>;

    async fn update_comment(
        &self,
        actor: &Identity,
        page_id: &str,
        comment_id: &str,
        body: &str,
    ) -> Result<Comment, crate::error::ProtocolError>;

    async fn resolve_comment(
        &self,
        actor: &Identity,
        page_id: &str,
        comment_id: &str,
    ) -> Result<Comment, crate::error::ProtocolError>;

    // ── suggestions ──

    async fn create_suggestion(
        &self,
        actor: &Identity,
        page_id: &str,
        kind: SuggestionKind,
        target_block_id: Option<&str>,
        new_content: &str,
        base_revisions: Vec<BaseRevision>,
    ) -> Result<Suggestion, crate::error::ProtocolError>;

    async fn accept_suggestion(
        &self,
        actor: &Identity,
        page_id: &str,
        suggestion_id: &str,
    ) -> Result<Suggestion, crate::error::ProtocolError>;

    async fn reject_suggestion(
        &self,
        actor: &Identity,
        page_id: &str,
        suggestion_id: &str,
    ) -> Result<Suggestion, crate::error::ProtocolError>;

    // ── history ──

    async fn list_history(
        &self,
        page_id: &str,
        category: HistoryCategory,
        from: Option<&str>,
        to: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AuditEvent>, crate::error::ProtocolError>;

    async fn history_diff(
        &self,
        page_id: &str,
        from_event_id: &str,
        to_event_id: &str,
    ) -> Result<HistoryDiff, crate::error::ProtocolError>;

    async fn set_audit_note(
        &self,
        actor: &Identity,
        page_id: &str,
        audit_event_id: &str,
        body: &str,
    ) -> Result<(), crate::error::ProtocolError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    InsertBlocks,
    ReplaceBlocks,
    DeleteBlocks,
}
