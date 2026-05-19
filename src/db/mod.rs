use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::Executor;
use sqlx::SqlitePool;
use std::str::FromStr;
use std::time::Duration as StdDuration;

mod page;
pub use page::*;

mod block;
pub use block::*;

mod comment;
pub use comment::*;

mod suggestion;
pub use suggestion::*;

mod lock;
pub use lock::*;

mod history;
pub use history::*;

mod audit;
pub(crate) use audit::*;

mod ops;

mod permission;
pub(crate) use permission::require_permission;

use crate::error::Result;

const AUDIT_NOTE_MAX_CHARS: usize = 2000;
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
        "CREATE TABLE IF NOT EXISTS pages (id TEXT PRIMARY KEY, title TEXT NOT NULL, properties_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS blocks (id TEXT PRIMARY KEY, page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE, parent_id TEXT REFERENCES blocks(id) ON DELETE CASCADE, order_index REAL NOT NULL, block_type TEXT NOT NULL, content_json TEXT NOT NULL DEFAULT '[]', properties_json TEXT NOT NULL DEFAULT '{}', revision INTEGER NOT NULL DEFAULT 1, deleted INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        "CREATE INDEX IF NOT EXISTS idx_blocks_page_order ON blocks(page_id, parent_id, deleted, order_index)",
        "CREATE INDEX IF NOT EXISTS idx_blocks_parent ON blocks(parent_id, deleted, order_index)",
        "CREATE TABLE IF NOT EXISTS block_locks (block_id TEXT PRIMARY KEY REFERENCES blocks(id) ON DELETE CASCADE, page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE, owner_client_id TEXT NOT NULL, owner_nickname TEXT NOT NULL, expires_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS comments (id TEXT PRIMARY KEY, page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE, target_block_id TEXT NOT NULL, start_column INTEGER, end_column INTEGER, author_client_id TEXT NOT NULL, author_nickname TEXT NOT NULL, role_mode TEXT NOT NULL, body TEXT NOT NULL, resolved INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS comment_replies (id TEXT PRIMARY KEY, comment_id TEXT NOT NULL REFERENCES comments(id) ON DELETE CASCADE, author_client_id TEXT NOT NULL, author_nickname TEXT NOT NULL, role_mode TEXT NOT NULL, body TEXT NOT NULL, created_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS suggestions (id TEXT PRIMARY KEY, page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE, kind TEXT NOT NULL, target_block_id TEXT, parent_id TEXT, content_json TEXT NOT NULL, base_revisions_json TEXT NOT NULL, state TEXT NOT NULL, author_client_id TEXT NOT NULL, author_nickname TEXT NOT NULL, role_mode TEXT NOT NULL, created_at TEXT NOT NULL, decided_by_client_id TEXT, decided_by_nickname TEXT, decided_at TEXT)",
        "CREATE TABLE IF NOT EXISTS audit_events (id TEXT PRIMARY KEY, page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE, actor_client_id TEXT NOT NULL, actor_nickname TEXT NOT NULL, role_mode TEXT NOT NULL, event_type TEXT NOT NULL, details_json TEXT NOT NULL, created_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS audit_event_notes (audit_event_id TEXT PRIMARY KEY REFERENCES audit_events(id) ON DELETE CASCADE, page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE, body TEXT NOT NULL, updated_by_client_id TEXT NOT NULL, updated_by_nickname TEXT NOT NULL, updated_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS page_versions (audit_event_id TEXT PRIMARY KEY REFERENCES audit_events(id) ON DELETE CASCADE, page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE, content TEXT NOT NULL, created_at TEXT NOT NULL)",
        "CREATE INDEX IF NOT EXISTS idx_page_versions_page ON page_versions(page_id, created_at)",
    ];
    for statement in schema {
        pool.execute(statement).await?;
    }
    Ok(())
}
