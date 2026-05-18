use documosa_core::error::ProtocolError;
use documosa_core::identity::Identity;
use documosa_core::models::*;
use documosa_core::operations::{DocumosaOps, SuggestionKind};
use sqlx::SqlitePool;

use crate::error::AppError;

use super::*;

/// Newtype wrapper around `SqlitePool` so we can implement `DocumosaOps`
/// without violating the Rust orphan rule (both `DocumosaOps` and `SqlitePool`
/// are foreign types).
#[derive(Clone)]
pub struct DocumosaDb(pub SqlitePool);

impl std::ops::Deref for DocumosaDb {
    type Target = SqlitePool;

    fn deref(&self) -> &SqlitePool {
        &self.0
    }
}

// Helper: convert AppError to ProtocolError.
// We cannot impl From<AppError> for ProtocolError because both From (std) and
// ProtocolError (documosa-core) are foreign to this crate — orphan rule.
fn into_protocol(err: AppError) -> ProtocolError {
    match err {
        AppError::Protocol(e) => e,
        AppError::BadRequest(msg) => ProtocolError::BadRequest(msg),
        AppError::Forbidden(msg) => ProtocolError::Forbidden(msg),
        AppError::Conflict(msg) => ProtocolError::Conflict(msg),
        AppError::NotFound => ProtocolError::NotFound,
        AppError::Unauthorized => ProtocolError::Forbidden("unauthorized".into()),
        AppError::Sqlx(sqlx::Error::RowNotFound) => ProtocolError::NotFound,
        _ => ProtocolError::BadRequest("internal error".into()),
    }
}

fn active_lines(snapshot: DocumentSnapshot) -> Vec<Line> {
    snapshot.lines.into_iter().filter(|l| !l.deleted).collect()
}

impl DocumosaOps for DocumosaDb {
    // ── document ──

    async fn create_document(
        &self,
        actor: &Identity,
        title: String,
        content: String,
    ) -> std::result::Result<DocumentSnapshot, ProtocolError> {
        document::create_document(&self.0, actor, title, content)
            .await
            .map_err(into_protocol)
    }

    async fn list_documents(
        &self,
    ) -> std::result::Result<Vec<Document>, ProtocolError> {
        document::list_documents(&self.0)
            .await
            .map_err(into_protocol)
    }

    async fn get_document(
        &self,
        document_id: &str,
    ) -> std::result::Result<DocumentSnapshot, ProtocolError> {
        document::snapshot(&self.0, document_id)
            .await
            .map_err(into_protocol)
    }

    async fn export_document(
        &self,
        document_id: &str,
    ) -> std::result::Result<String, ProtocolError> {
        document::export_document(&self.0, document_id)
            .await
            .map_err(into_protocol)
    }

    async fn update_title(
        &self,
        actor: &Identity,
        document_id: &str,
        title: &str,
    ) -> std::result::Result<(), ProtocolError> {
        document::update_document_title(&self.0, actor, document_id, title)
            .await
            .map_err(into_protocol)
    }

    // ── lines ──

    async fn insert_lines(
        &self,
        actor: &Identity,
        document_id: &str,
        anchor_line_id: Option<&str>,
        lines: Vec<String>,
    ) -> std::result::Result<Vec<Line>, ProtocolError> {
        let after = anchor_line_id.map(|s| s.to_string());
        let snapshot = line::insert_lines(&self.0, actor, document_id, after, lines)
            .await
            .map_err(into_protocol)?;
        Ok(active_lines(snapshot))
    }

    async fn replace_lines(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
        new_contents: Vec<String>,
        _base_revisions: Vec<BaseRevision>,
    ) -> std::result::Result<Vec<Line>, ProtocolError> {
        let ids: Vec<String> = line_ids.into_iter().map(|s| s.to_string()).collect();
        let snapshot = line::replace_lines(&self.0, actor, document_id, ids, new_contents)
            .await
            .map_err(into_protocol)?;
        Ok(active_lines(snapshot))
    }

    async fn delete_lines(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
    ) -> std::result::Result<Vec<Line>, ProtocolError> {
        let ids: Vec<String> = line_ids.into_iter().map(|s| s.to_string()).collect();
        let snapshot = line::delete_lines(&self.0, actor, document_id, ids)
            .await
            .map_err(into_protocol)?;
        Ok(active_lines(snapshot))
    }

    // ── locks ──

    async fn lock_lines(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
        _ttl_seconds: i64,
    ) -> std::result::Result<Vec<LineLock>, ProtocolError> {
        let ids: Vec<String> = line_ids.into_iter().map(|s| s.to_string()).collect();
        lock::heartbeat_locks(&self.0, actor, document_id, ids)
            .await
            .map_err(into_protocol)
    }

    async fn heartbeat_locks(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
    ) -> std::result::Result<Vec<LineLock>, ProtocolError> {
        let ids: Vec<String> = line_ids.into_iter().map(|s| s.to_string()).collect();
        lock::heartbeat_locks(&self.0, actor, document_id, ids)
            .await
            .map_err(into_protocol)
    }

    async fn release_locks(
        &self,
        actor: &Identity,
        document_id: &str,
        line_ids: Vec<&str>,
    ) -> std::result::Result<(), ProtocolError> {
        let ids: Vec<String> = line_ids.into_iter().map(|s| s.to_string()).collect();
        lock::release_locks(&self.0, actor, document_id, ids)
            .await
            .map_err(into_protocol)?;
        Ok(())
    }

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
    ) -> std::result::Result<Comment, ProtocolError> {
        let draft = comment::CommentDraft {
            start_line_id: start_line_id.to_string(),
            end_line_id: end_line_id.to_string(),
            start_column,
            end_column,
            body: body.to_string(),
        };
        let snapshot = comment::create_comment(&self.0, actor, document_id, draft)
            .await
            .map_err(into_protocol)?;
        snapshot
            .comments
            .into_iter()
            .next()
            .ok_or(ProtocolError::NotFound)
    }

    async fn reply_comment(
        &self,
        actor: &Identity,
        document_id: &str,
        comment_id: &str,
        body: &str,
    ) -> std::result::Result<CommentReply, ProtocolError> {
        let snapshot =
            comment::reply_comment(&self.0, actor, document_id, comment_id, body.to_string())
                .await
                .map_err(into_protocol)?;
        snapshot
            .replies
            .into_iter()
            .last()
            .ok_or(ProtocolError::NotFound)
    }

    async fn update_comment(
        &self,
        actor: &Identity,
        document_id: &str,
        comment_id: &str,
        body: &str,
    ) -> std::result::Result<Comment, ProtocolError> {
        let snapshot =
            comment::update_comment(&self.0, actor, document_id, comment_id, body.to_string())
                .await
                .map_err(into_protocol)?;
        snapshot
            .comments
            .into_iter()
            .find(|c| c.id == comment_id)
            .ok_or(ProtocolError::NotFound)
    }

    async fn resolve_comment(
        &self,
        actor: &Identity,
        document_id: &str,
        comment_id: &str,
    ) -> std::result::Result<Comment, ProtocolError> {
        let snapshot = comment::resolve_comment(&self.0, actor, document_id, comment_id)
            .await
            .map_err(into_protocol)?;
        snapshot
            .comments
            .into_iter()
            .find(|c| c.id == comment_id)
            .ok_or(ProtocolError::NotFound)
    }

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
        _base_revisions: Vec<BaseRevision>,
    ) -> std::result::Result<Suggestion, ProtocolError> {
        let kind_str = match kind {
            SuggestionKind::InsertLines => "insert",
            SuggestionKind::ReplaceLines => "replace",
            SuggestionKind::DeleteLines => "delete",
        };
        let anchor = anchor_line_id.map(|s| s.to_string());
        let start = start_line_id.map(|s| s.to_string());
        let end = end_line_id.map(|s| s.to_string());
        let content: Vec<String> = new_content.split('\n').map(|s| s.to_string()).collect();
        let snapshot = suggestion::create_suggestion(
            &self.0,
            actor,
            document_id,
            kind_str.to_string(),
            anchor,
            start,
            end,
            content,
        )
        .await
        .map_err(into_protocol)?;
        snapshot
            .suggestions
            .into_iter()
            .last()
            .ok_or(ProtocolError::NotFound)
    }

    async fn accept_suggestion(
        &self,
        actor: &Identity,
        document_id: &str,
        suggestion_id: &str,
    ) -> std::result::Result<Suggestion, ProtocolError> {
        let snapshot =
            suggestion::decide_suggestion(&self.0, actor, document_id, suggestion_id, true)
                .await
                .map_err(into_protocol)?;
        snapshot
            .suggestions
            .into_iter()
            .find(|s| s.id == suggestion_id)
            .ok_or(ProtocolError::NotFound)
    }

    async fn reject_suggestion(
        &self,
        actor: &Identity,
        document_id: &str,
        suggestion_id: &str,
    ) -> std::result::Result<Suggestion, ProtocolError> {
        let snapshot =
            suggestion::decide_suggestion(&self.0, actor, document_id, suggestion_id, false)
                .await
                .map_err(into_protocol)?;
        snapshot
            .suggestions
            .into_iter()
            .find(|s| s.id == suggestion_id)
            .ok_or(ProtocolError::NotFound)
    }

    // ── history ──

    async fn list_history(
        &self,
        document_id: &str,
        category: HistoryCategory,
        from: Option<&str>,
        to: Option<&str>,
        limit: i64,
    ) -> std::result::Result<Vec<AuditEvent>, ProtocolError> {
        let options = HistoryListOptions {
            category,
            from: from.map(|s| s.to_string()),
            to: to.map(|s| s.to_string()),
            limit,
        };
        history::list_history_events(&self.0, document_id, options)
            .await
            .map_err(into_protocol)
    }

    async fn history_diff(
        &self,
        document_id: &str,
        from_event_id: &str,
        to_event_id: &str,
    ) -> std::result::Result<HistoryDiff, ProtocolError> {
        history::history_diff(&self.0, document_id, from_event_id, to_event_id)
            .await
            .map_err(into_protocol)
    }

    async fn set_audit_note(
        &self,
        actor: &Identity,
        document_id: &str,
        audit_event_id: &str,
        body: &str,
    ) -> std::result::Result<(), ProtocolError> {
        history::put_audit_event_note(&self.0, actor, document_id, audit_event_id, body.to_string())
            .await
            .map_err(into_protocol)?;
        Ok(())
    }
}
