use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::Executor;
use sqlx::SqlitePool;
use std::str::FromStr;
use std::time::Duration as StdDuration;

mod ops;
pub use ops::*;

mod text;
pub(crate) use text::*;

mod audit;
pub(crate) use audit::{audit_tx, begin_write_tx, touch_document_tx, prune_expired_locks_tx};

mod permission;
pub(crate) use permission::require_permission;

mod document;
pub use document::{
    create_document, export_document, list_documents, snapshot, update_document_title,
};

mod line;
pub use line::{delete_lines, insert_lines, replace_lines, update_content};

mod lock;
pub use lock::{heartbeat_locks, locks, release_locks};

mod comment;
pub use comment::{
    CommentDraft, create_comment, delete_comment, reply_comment, resolve_comment, update_comment,
};

mod suggestion;
pub use suggestion::{create_suggestion, decide_suggestion};

mod history;
pub use history::{history_diff, list_history_events, put_audit_event_note};

pub(crate) use line::{
    active_lines_tx, ensure_line_exists_tx, get_line_tx, insert_line_at,
};

use crate::error::Result;

const AUDIT_NOTE_MAX_CHARS: usize = 2000;
pub(crate) const LINE_ORDER_STEP: i64 = 1000;
pub const HISTORY_DEFAULT_LIMIT: i64 = 200;
pub const HISTORY_MAX_LIMIT: i64 = 500;

pub async fn connect(path: &Path) -> anyhow::Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(StdDuration::from_secs(5))
        .foreign_keys(true);
    Ok(SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?)
}

pub async fn connect_memory() -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")?
        .create_if_missing(true)
        .busy_timeout(StdDuration::from_secs(5))
        .foreign_keys(true);
    Ok(SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?)
}

pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    let schema = [
        "CREATE TABLE IF NOT EXISTS documents (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS lines (id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE, order_index INTEGER NOT NULL, content TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1, deleted INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        "CREATE INDEX IF NOT EXISTS idx_lines_document_order ON lines(document_id, deleted, order_index)",
        "CREATE TABLE IF NOT EXISTS locks (line_id TEXT PRIMARY KEY REFERENCES lines(id) ON DELETE CASCADE, document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE, owner_client_id TEXT NOT NULL, owner_nickname TEXT NOT NULL, expires_at TEXT NOT NULL)",
        "CREATE INDEX IF NOT EXISTS idx_locks_document ON locks(document_id)",
        "CREATE TABLE IF NOT EXISTS comments (id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE, start_line_id TEXT NOT NULL, end_line_id TEXT NOT NULL, author_client_id TEXT NOT NULL, author_nickname TEXT NOT NULL, role_mode TEXT NOT NULL, body TEXT NOT NULL, resolved INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS comment_replies (id TEXT PRIMARY KEY, comment_id TEXT NOT NULL REFERENCES comments(id) ON DELETE CASCADE, author_client_id TEXT NOT NULL, author_nickname TEXT NOT NULL, role_mode TEXT NOT NULL, body TEXT NOT NULL, created_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS suggestions (id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE, kind TEXT NOT NULL, anchor_line_id TEXT, start_line_id TEXT, end_line_id TEXT, content_json TEXT NOT NULL, base_revisions_json TEXT NOT NULL, state TEXT NOT NULL, author_client_id TEXT NOT NULL, author_nickname TEXT NOT NULL, role_mode TEXT NOT NULL, created_at TEXT NOT NULL, decided_by_client_id TEXT, decided_by_nickname TEXT, decided_at TEXT)",
        "CREATE TABLE IF NOT EXISTS audit_events (id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE, actor_client_id TEXT NOT NULL, actor_nickname TEXT NOT NULL, role_mode TEXT NOT NULL, event_type TEXT NOT NULL, details_json TEXT NOT NULL, created_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS audit_event_notes (audit_event_id TEXT PRIMARY KEY REFERENCES audit_events(id) ON DELETE CASCADE, document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE, body TEXT NOT NULL, updated_by_client_id TEXT NOT NULL, updated_by_nickname TEXT NOT NULL, updated_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS document_versions (audit_event_id TEXT PRIMARY KEY REFERENCES audit_events(id) ON DELETE CASCADE, document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE, content TEXT NOT NULL, created_at TEXT NOT NULL)",
        "CREATE INDEX IF NOT EXISTS idx_document_versions_document ON document_versions(document_id, created_at)",
    ];
    for statement in schema {
        pool.execute(statement).await?;
    }
    let has_start_column: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('comments') WHERE name = 'start_column'",
    )
    .fetch_one(pool)
    .await?;
    if has_start_column.0 == 0 {
        pool.execute("ALTER TABLE comments ADD COLUMN start_column INTEGER")
            .await?;
    }
    let has_end_column: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('comments') WHERE name = 'end_column'",
    )
    .fetch_one(pool)
    .await?;
    if has_end_column.0 == 0 {
        pool.execute("ALTER TABLE comments ADD COLUMN end_column INTEGER")
            .await?;
    }
    Ok(())
}
