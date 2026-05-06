use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Executor, QueryBuilder, Sqlite, SqlitePool, Transaction};
use std::str::FromStr;
use std::time::Duration as StdDuration;

use crate::error::{AppError, Result};
use crate::models::*;

const AUDIT_NOTE_MAX_CHARS: usize = 2000;
const LINE_ORDER_STEP: i64 = 1000;
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

pub async fn create_document(
    pool: &SqlitePool,
    actor: &Identity,
    title: String,
    content: String,
) -> Result<DocumentSnapshot> {
    let mut tx = begin_write_tx(pool).await?;
    let timestamp = now();
    let document = Document {
        id: new_id(),
        title,
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
    };
    sqlx::query("INSERT INTO documents (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&document.id)
        .bind(&document.title)
        .bind(&document.created_at)
        .bind(&document.updated_at)
        .execute(&mut *tx)
        .await?;
    let lines = split_lines_preserve_trailing(&content);
    for (index, line) in lines.iter().enumerate() {
        insert_line_at(
            &mut tx,
            &document.id,
            (index as i64 + 1) * LINE_ORDER_STEP,
            line,
        )
        .await?;
    }
    audit_tx(
        &mut tx,
        &document.id,
        actor,
        "document.created",
        json!({
            "title": document.title,
            "initial_content_summary": text_summary(&content),
            "initial_content_length": text_len(&content),
            "line_count": line_count(&content),
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, &document.id).await
}

pub async fn list_documents(pool: &SqlitePool) -> Result<Vec<Document>> {
    Ok(sqlx::query_as::<_, Document>(
        "SELECT id, title, created_at, updated_at FROM documents ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn update_document_title(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    title: &str,
) -> Result<()> {
    let mut tx = begin_write_tx(pool).await?;
    let timestamp = now();
    sqlx::query("UPDATE documents SET title = ?, updated_at = ? WHERE id = ?")
        .bind(title)
        .bind(&timestamp)
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "document.title_updated",
        json!({ "title": title }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn snapshot(pool: &SqlitePool, document_id: &str) -> Result<DocumentSnapshot> {
    let timestamp = now();
    let document = sqlx::query_as::<_, Document>(
        "SELECT id, title, created_at, updated_at FROM documents WHERE id = ?",
    )
    .bind(document_id)
    .fetch_one(pool)
    .await?;
    let lines = sqlx::query_as::<_, Line>(
        "SELECT id, document_id, order_index, content, revision, deleted, created_at, updated_at FROM lines WHERE document_id = ? ORDER BY order_index, created_at",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, document_id, start_line_id, end_line_id, start_column, end_column, author_client_id, author_nickname, role_mode, body, resolved, created_at, updated_at FROM comments WHERE document_id = ? ORDER BY created_at",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    let replies = sqlx::query_as::<_, CommentReply>(
        "SELECT r.id, r.comment_id, r.author_client_id, r.author_nickname, r.role_mode, r.body, r.created_at FROM comment_replies r JOIN comments c ON c.id = r.comment_id WHERE c.document_id = ? ORDER BY r.created_at",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    let suggestions = sqlx::query_as::<_, Suggestion>(
        "SELECT id, document_id, kind, anchor_line_id, start_line_id, end_line_id, content_json, base_revisions_json, state, author_client_id, author_nickname, role_mode, created_at, decided_by_client_id, decided_by_nickname, decided_at FROM suggestions WHERE document_id = ? ORDER BY created_at",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    let locks = sqlx::query_as::<_, LineLock>(
        "SELECT line_id, document_id, owner_client_id, owner_nickname, expires_at FROM locks WHERE document_id = ? AND expires_at > ? ORDER BY expires_at",
    )
    .bind(document_id)
    .bind(&timestamp)
    .fetch_all(pool)
    .await?;
    let audit_events = sqlx::query_as::<_, AuditEvent>(
        "SELECT a.id, a.document_id, a.actor_client_id, a.actor_nickname, a.role_mode, a.event_type, a.details_json, a.created_at, n.body AS note_body, n.updated_by_nickname AS note_updated_by_nickname, n.updated_at AS note_updated_at FROM audit_events a LEFT JOIN audit_event_notes n ON n.audit_event_id = a.id WHERE a.document_id = ? ORDER BY a.created_at DESC LIMIT 200",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    Ok(DocumentSnapshot {
        document,
        lines,
        comments,
        replies,
        suggestions,
        locks,
        audit_events,
    })
}

pub async fn list_history_events(
    pool: &SqlitePool,
    document_id: &str,
    options: HistoryListOptions,
) -> Result<Vec<AuditEvent>> {
    if options.limit < 1 || options.limit > HISTORY_MAX_LIMIT {
        return Err(AppError::BadRequest(format!(
            "history limit must be between 1 and {HISTORY_MAX_LIMIT}"
        )));
    }
    ensure_document_exists(pool, document_id).await?;

    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT a.id, a.document_id, a.actor_client_id, a.actor_nickname, a.role_mode, a.event_type, a.details_json, a.created_at, n.body AS note_body, n.updated_by_nickname AS note_updated_by_nickname, n.updated_at AS note_updated_at FROM audit_events a LEFT JOIN audit_event_notes n ON n.audit_event_id = a.id WHERE a.document_id = ",
    );
    builder.push_bind(document_id);
    push_history_category_filter(&mut builder, options.category);
    if let Some(from) = options.from {
        builder.push(" AND a.created_at >= ");
        builder.push_bind(from);
    }
    if let Some(to) = options.to {
        builder.push(" AND a.created_at <= ");
        builder.push_bind(to);
    }
    builder.push(" ORDER BY a.created_at DESC LIMIT ");
    builder.push_bind(options.limit);
    Ok(builder
        .build_query_as::<AuditEvent>()
        .fetch_all(pool)
        .await?)
}

pub async fn put_audit_event_note(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    audit_event_id: &str,
    body: String,
) -> Result<DocumentSnapshot> {
    let trimmed = body.trim().to_string();
    if trimmed.chars().count() > AUDIT_NOTE_MAX_CHARS {
        return Err(AppError::BadRequest(format!(
            "audit note must be at most {AUDIT_NOTE_MAX_CHARS} characters"
        )));
    }

    let mut tx = begin_write_tx(pool).await?;
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM audit_events WHERE id = ? AND document_id = ?")
            .bind(audit_event_id)
            .bind(document_id)
            .fetch_one(&mut *tx)
            .await?;
    if count.0 == 0 {
        return Err(AppError::NotFound);
    }

    if trimmed.is_empty() {
        sqlx::query("DELETE FROM audit_event_notes WHERE audit_event_id = ? AND document_id = ?")
            .bind(audit_event_id)
            .bind(document_id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("INSERT INTO audit_event_notes (audit_event_id, document_id, body, updated_by_client_id, updated_by_nickname, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(audit_event_id) DO UPDATE SET document_id = excluded.document_id, body = excluded.body, updated_by_client_id = excluded.updated_by_client_id, updated_by_nickname = excluded.updated_by_nickname, updated_at = excluded.updated_at")
            .bind(audit_event_id)
            .bind(document_id)
            .bind(&trimmed)
            .bind(&actor.client_id)
            .bind(&actor.nickname)
            .bind(now())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    snapshot(pool, document_id).await
}

pub async fn export_document(pool: &SqlitePool, document_id: &str) -> Result<String> {
    sqlx::query_as::<_, Document>(
        "SELECT id, title, created_at, updated_at FROM documents WHERE id = ?",
    )
    .bind(document_id)
    .fetch_one(pool)
    .await?;
    let lines = sqlx::query_as::<_, Line>(
        "SELECT id, document_id, order_index, content, revision, deleted, created_at, updated_at FROM lines WHERE document_id = ? AND deleted = 0 ORDER BY order_index, created_at",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    Ok(lines
        .into_iter()
        .map(|line| line.content)
        .collect::<Vec<_>>()
        .join("\n"))
}

pub async fn history_diff(
    pool: &SqlitePool,
    document_id: &str,
    from_audit_event_id: &str,
    to_audit_event_id: &str,
) -> Result<HistoryDiff> {
    let from_event = audit_event(pool, document_id, from_audit_event_id).await?;
    let to_event = audit_event(pool, document_id, to_audit_event_id).await?;
    let from_content = version_content(pool, document_id, from_audit_event_id).await?;
    let to_content = version_content(pool, document_id, to_audit_event_id).await?;
    Ok(HistoryDiff {
        from_event,
        to_event,
        from_content,
        to_content,
    })
}

pub async fn update_content(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    content: String,
    base_revisions: Vec<BaseRevision>,
) -> Result<DocumentSnapshot> {
    require_writer(actor)?;
    let mut tx = begin_write_tx(pool).await?;
    let current = active_lines_tx(&mut tx, document_id).await?;
    if current.len() != base_revisions.len()
        || current
            .iter()
            .zip(base_revisions.iter())
            .any(|(line, base)| line.id != base.line_id || line.revision != base.revision)
    {
        return Err(AppError::Conflict(
            "document content is based on stale revisions".into(),
        ));
    }

    let next_content: Vec<String> = if content.is_empty() {
        Vec::new()
    } else {
        content.split('\n').map(ToString::to_string).collect()
    };

    let mut prefix_len = 0;
    while prefix_len < current.len()
        && prefix_len < next_content.len()
        && current[prefix_len].content == next_content[prefix_len]
    {
        prefix_len += 1;
    }

    let mut suffix_len = 0;
    while suffix_len < current.len().saturating_sub(prefix_len)
        && suffix_len < next_content.len().saturating_sub(prefix_len)
        && current[current.len() - 1 - suffix_len].content
            == next_content[next_content.len() - 1 - suffix_len]
    {
        suffix_len += 1;
    }

    let old_mid_end = current.len() - suffix_len;
    let new_mid_end = next_content.len() - suffix_len;

    let mut touched_ids: Vec<String> = current[prefix_len..old_mid_end]
        .iter()
        .map(|line| line.id.clone())
        .collect();
    touched_ids.sort();
    ensure_unlocked_tx(&mut tx, actor, document_id, &touched_ids).await?;

    let timestamp = now();
    let mut deleted_lines = Vec::new();
    let mut inserted_lines = Vec::new();

    for old_line in &current[prefix_len..old_mid_end] {
        sqlx::query(
            "UPDATE lines SET deleted = 1, revision = revision + 1, updated_at = ? WHERE id = ? AND document_id = ? AND deleted = 0",
        )
        .bind(&timestamp)
        .bind(&old_line.id)
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
        deleted_lines.push(json!({
            "line_id": old_line.id,
            "content_summary": text_summary(&old_line.content),
            "content_length": text_len(&old_line.content),
        }));
    }

    let previous_order = current.last().map_or(0, |line| line.order_index) + 1_000_000;
    for (index, value) in next_content[prefix_len..new_mid_end].iter().enumerate() {
        let line = insert_line_at(
            &mut tx,
            document_id,
            previous_order + ((index as i64 + 1) * LINE_ORDER_STEP),
            value,
        )
        .await?;
        inserted_lines.push(json!({
            "line_id": line.id,
            "content_summary": text_summary(&line.content),
            "content_length": text_len(&line.content),
        }));
    }
    let deleted_ids: Vec<String> = deleted_lines
        .iter()
        .filter_map(|line| line["line_id"].as_str().map(ToString::to_string))
        .collect();
    let inserted_ids: Vec<String> = inserted_lines
        .iter()
        .filter_map(|line| line["line_id"].as_str().map(ToString::to_string))
        .collect();

    let final_ids: Vec<String> = current[..prefix_len]
        .iter()
        .map(|line| line.id.clone())
        .chain(inserted_ids.iter().cloned())
        .chain(current[old_mid_end..].iter().map(|line| line.id.clone()))
        .collect();
    for (index, id) in final_ids.into_iter().enumerate() {
        sqlx::query("UPDATE lines SET order_index = ? WHERE id = ? AND document_id = ?")
            .bind((index as i64 + 1) * LINE_ORDER_STEP)
            .bind(id)
            .bind(document_id)
            .execute(&mut *tx)
            .await?;
    }
    touch_document_tx(&mut tx, document_id).await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "document.content_updated",
        json!({
            "deleted_line_ids": deleted_ids,
            "inserted_line_ids": inserted_ids,
            "deleted_lines": deleted_lines,
            "inserted_lines": inserted_lines,
            "deleted_count": deleted_ids.len(),
            "inserted_count": inserted_ids.len(),
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn insert_lines(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    after_line_id: Option<String>,
    content: Vec<String>,
) -> Result<DocumentSnapshot> {
    require_writer(actor)?;
    if content.is_empty() {
        return Err(AppError::BadRequest("at least one line is required".into()));
    }
    let mut tx = begin_write_tx(pool).await?;
    if let Some(anchor) = &after_line_id {
        ensure_unlocked_tx(&mut tx, actor, document_id, std::slice::from_ref(anchor)).await?;
    }
    let orders = insertion_orders_tx(
        &mut tx,
        document_id,
        after_line_id.as_deref(),
        content.len(),
    )
    .await?;
    let mut inserted = Vec::new();
    for (order, value) in orders.into_iter().zip(content.iter()) {
        inserted.push(insert_line_at(&mut tx, document_id, order, value).await?);
    }
    let inserted_details: Vec<_> = inserted
        .iter()
        .map(|line| {
            json!({
                "line_id": line.id,
                "content_summary": text_summary(&line.content),
                "content_length": text_len(&line.content),
            })
        })
        .collect();
    let line_ids: Vec<_> = inserted.iter().map(|line| line.id.clone()).collect();
    touch_document_tx(&mut tx, document_id).await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "lines.inserted",
        json!({
            "line_ids": line_ids,
            "lines": inserted_details,
            "count": inserted.len(),
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn replace_lines(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    line_ids: Vec<String>,
    content: Vec<String>,
) -> Result<DocumentSnapshot> {
    require_writer(actor)?;
    if line_ids.is_empty() || line_ids.len() != content.len() {
        return Err(AppError::BadRequest(
            "line_ids and content must be non-empty and equal length".into(),
        ));
    }
    let mut targets = line_ids.clone();
    targets.sort();
    let mut tx = begin_write_tx(pool).await?;
    ensure_unlocked_tx(&mut tx, actor, document_id, &targets).await?;
    let timestamp = now();
    let mut replacement_details = Vec::new();
    for (line_id, value) in line_ids.iter().zip(content.iter()) {
        let before = get_line_tx(&mut tx, document_id, line_id).await?;
        let affected = sqlx::query(
            "UPDATE lines SET content = ?, revision = revision + 1, updated_at = ? WHERE id = ? AND document_id = ? AND deleted = 0",
        )
        .bind(value)
        .bind(&timestamp)
        .bind(line_id)
        .bind(document_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if affected != 1 {
            return Err(AppError::NotFound);
        }
        replacement_details.push(json!({
            "line_id": line_id,
            "before_summary": text_summary(&before.content),
            "before_length": text_len(&before.content),
            "after_summary": text_summary(value),
            "after_length": text_len(value),
        }));
    }
    touch_document_tx(&mut tx, document_id).await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "lines.replaced",
        json!({
            "line_ids": line_ids,
            "lines": replacement_details,
            "count": replacement_details.len(),
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn delete_lines(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    line_ids: Vec<String>,
) -> Result<DocumentSnapshot> {
    require_writer(actor)?;
    if line_ids.is_empty() {
        return Err(AppError::BadRequest("line_ids are required".into()));
    }
    let mut targets = line_ids.clone();
    targets.sort();
    let mut tx = begin_write_tx(pool).await?;
    ensure_unlocked_tx(&mut tx, actor, document_id, &targets).await?;
    let timestamp = now();
    let mut deleted_details = Vec::new();
    for line_id in &line_ids {
        let before = get_line_tx(&mut tx, document_id, line_id).await?;
        let affected = sqlx::query(
            "UPDATE lines SET deleted = 1, revision = revision + 1, updated_at = ? WHERE id = ? AND document_id = ? AND deleted = 0",
        )
        .bind(&timestamp)
        .bind(line_id)
        .bind(document_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if affected != 1 {
            return Err(AppError::NotFound);
        }
        deleted_details.push(json!({
            "line_id": line_id,
            "content_summary": text_summary(&before.content),
            "content_length": text_len(&before.content),
        }));
    }
    touch_document_tx(&mut tx, document_id).await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "lines.deleted",
        json!({
            "line_ids": line_ids,
            "lines": deleted_details,
            "count": deleted_details.len(),
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn heartbeat_locks(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    line_ids: Vec<String>,
) -> Result<Vec<LineLock>> {
    if line_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut tx = begin_write_tx(pool).await?;
    ensure_unlocked_tx(&mut tx, actor, document_id, &line_ids).await?;
    let expires_at = (Utc::now() + Duration::seconds(60)).to_rfc3339();
    for line_id in &line_ids {
        sqlx::query(
            "INSERT INTO locks (line_id, document_id, owner_client_id, owner_nickname, expires_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(line_id) DO UPDATE SET owner_client_id = excluded.owner_client_id, owner_nickname = excluded.owner_nickname, expires_at = excluded.expires_at",
        )
        .bind(line_id)
        .bind(document_id)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(&expires_at)
        .execute(&mut *tx)
        .await?;
    }
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "locks.heartbeat",
        json!({ "line_ids": line_ids }),
    )
    .await?;
    tx.commit().await?;
    locks(pool, document_id).await
}

pub async fn release_locks(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    line_ids: Vec<String>,
) -> Result<Vec<LineLock>> {
    let mut tx = begin_write_tx(pool).await?;
    prune_expired_locks_tx(&mut tx).await?;
    let mut builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("DELETE FROM locks WHERE document_id = ");
    builder.push_bind(document_id);
    builder.push(" AND owner_client_id = ");
    builder.push_bind(&actor.client_id);
    if !line_ids.is_empty() {
        builder.push(" AND line_id IN (");
        let mut separated = builder.separated(", ");
        for id in line_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
    }
    builder.build().execute(&mut *tx).await?;
    tx.commit().await?;
    locks(pool, document_id).await
}

pub async fn locks(pool: &SqlitePool, document_id: &str) -> Result<Vec<LineLock>> {
    let timestamp = now();
    Ok(sqlx::query_as::<_, LineLock>(
        "SELECT line_id, document_id, owner_client_id, owner_nickname, expires_at FROM locks WHERE document_id = ? AND expires_at > ? ORDER BY expires_at",
    )
    .bind(document_id)
    .bind(&timestamp)
    .fetch_all(pool)
    .await?)
}

pub struct CommentDraft {
    pub start_line_id: String,
    pub end_line_id: String,
    pub start_column: Option<i64>,
    pub end_column: Option<i64>,
    pub body: String,
}

pub async fn create_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    draft: CommentDraft,
) -> Result<DocumentSnapshot> {
    if draft.body.trim().is_empty() {
        return Err(AppError::BadRequest("comment body is required".into()));
    }
    let mut tx = begin_write_tx(pool).await?;
    ensure_line_exists_tx(&mut tx, document_id, &draft.start_line_id).await?;
    ensure_line_exists_tx(&mut tx, document_id, &draft.end_line_id).await?;
    if draft.start_column.is_some_and(|column| column < 0)
        || draft.end_column.is_some_and(|column| column < 0)
    {
        return Err(AppError::BadRequest(
            "comment columns must be non-negative".into(),
        ));
    }
    if draft.start_line_id == draft.end_line_id
        && let (Some(start), Some(end)) = (draft.start_column, draft.end_column)
        && end <= start
    {
        return Err(AppError::BadRequest(
            "comment selection must not be empty".into(),
        ));
    }
    let timestamp = now();
    let id = new_id();
    sqlx::query("INSERT INTO comments (id, document_id, start_line_id, end_line_id, start_column, end_column, author_client_id, author_nickname, role_mode, body, resolved, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)")
        .bind(&id)
        .bind(document_id)
        .bind(&draft.start_line_id)
        .bind(&draft.end_line_id)
        .bind(draft.start_column)
        .bind(draft.end_column)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(actor.role_mode.as_str())
        .bind(&draft.body)
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "comment.created",
        json!({
            "comment_id": id,
            "start_line_id": draft.start_line_id,
            "end_line_id": draft.end_line_id,
            "start_column": draft.start_column,
            "end_column": draft.end_column,
            "body": draft.body,
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn update_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    comment_id: &str,
    body: String,
) -> Result<DocumentSnapshot> {
    let mut tx = begin_write_tx(pool).await?;
    let existing = get_comment_tx(&mut tx, document_id, comment_id).await?;
    if actor.role_mode == RoleMode::Reviewer && existing.author_client_id != actor.client_id {
        return Err(AppError::Forbidden(
            "reviewers may update only their own comments".into(),
        ));
    }
    let timestamp = now();
    sqlx::query("UPDATE comments SET body = ?, updated_at = ? WHERE id = ? AND document_id = ?")
        .bind(&body)
        .bind(&timestamp)
        .bind(comment_id)
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "comment.updated",
        json!({
            "comment_id": comment_id,
            "before_body": existing.body,
            "after_body": body,
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn delete_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    comment_id: &str,
) -> Result<DocumentSnapshot> {
    let mut tx = begin_write_tx(pool).await?;
    let existing = get_comment_tx(&mut tx, document_id, comment_id).await?;
    if actor.role_mode == RoleMode::Reviewer && existing.author_client_id != actor.client_id {
        return Err(AppError::Forbidden(
            "reviewers may delete only their own comments".into(),
        ));
    }
    sqlx::query("DELETE FROM comments WHERE id = ? AND document_id = ?")
        .bind(comment_id)
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "comment.deleted",
        json!({
            "comment_id": comment_id,
            "body": existing.body,
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn reply_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    comment_id: &str,
    body: String,
) -> Result<DocumentSnapshot> {
    let mut tx = begin_write_tx(pool).await?;
    let comment = get_comment_tx(&mut tx, document_id, comment_id).await?;
    let id = new_id();
    let timestamp = now();
    sqlx::query("INSERT INTO comment_replies (id, comment_id, author_client_id, author_nickname, role_mode, body, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(comment_id)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(actor.role_mode.as_str())
        .bind(&body)
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "comment.replied",
        json!({
            "comment_id": comment_id,
            "reply_id": id,
            "body": body,
            "comment_body": comment.body,
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn resolve_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    comment_id: &str,
) -> Result<DocumentSnapshot> {
    require_writer(actor)?;
    let mut tx = begin_write_tx(pool).await?;
    let existing = get_comment_tx(&mut tx, document_id, comment_id).await?;
    let timestamp = now();
    let affected = sqlx::query(
        "UPDATE comments SET resolved = 1, updated_at = ? WHERE id = ? AND document_id = ?",
    )
    .bind(&timestamp)
    .bind(comment_id)
    .bind(document_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected != 1 {
        return Err(AppError::NotFound);
    }
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "comment.resolved",
        json!({
            "comment_id": comment_id,
            "body": existing.body,
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

#[allow(clippy::too_many_arguments)]
pub async fn create_suggestion(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    kind: String,
    anchor_line_id: Option<String>,
    start_line_id: Option<String>,
    end_line_id: Option<String>,
    content: Vec<String>,
) -> Result<DocumentSnapshot> {
    if !matches!(kind.as_str(), "insert" | "replace" | "delete") {
        return Err(AppError::BadRequest(
            "suggestion kind must be insert, replace, or delete".into(),
        ));
    }
    let mut tx = begin_write_tx(pool).await?;
    let mut base_revisions = Vec::new();
    for id in [&anchor_line_id, &start_line_id, &end_line_id]
        .into_iter()
        .flatten()
    {
        let line = get_line_tx(&mut tx, document_id, id).await?;
        base_revisions.push(BaseRevision {
            line_id: line.id,
            revision: line.revision,
        });
    }
    let id = new_id();
    let timestamp = now();
    sqlx::query("INSERT INTO suggestions (id, document_id, kind, anchor_line_id, start_line_id, end_line_id, content_json, base_revisions_json, state, author_client_id, author_nickname, role_mode, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'open', ?, ?, ?, ?)")
        .bind(&id)
        .bind(document_id)
        .bind(&kind)
        .bind(&anchor_line_id)
        .bind(&start_line_id)
        .bind(&end_line_id)
        .bind(serde_json::to_string(&content)?)
        .bind(serde_json::to_string(&base_revisions)?)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(actor.role_mode.as_str())
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        "suggestion.created",
        json!({ "suggestion_id": id, "kind": kind }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

pub async fn decide_suggestion(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    suggestion_id: &str,
    accept: bool,
) -> Result<DocumentSnapshot> {
    require_writer(actor)?;
    let mut tx = begin_write_tx(pool).await?;
    let suggestion = sqlx::query_as::<_, Suggestion>(
        "SELECT id, document_id, kind, anchor_line_id, start_line_id, end_line_id, content_json, base_revisions_json, state, author_client_id, author_nickname, role_mode, created_at, decided_by_client_id, decided_by_nickname, decided_at FROM suggestions WHERE id = ? AND document_id = ?",
    )
    .bind(suggestion_id)
    .bind(document_id)
    .fetch_one(&mut *tx)
    .await?;
    if suggestion.state != "open" {
        return Err(AppError::Conflict("suggestion is already decided".into()));
    }
    if accept {
        let revisions: Vec<BaseRevision> = serde_json::from_str(&suggestion.base_revisions_json)?;
        for expected in revisions {
            let line = get_line_tx(&mut tx, document_id, &expected.line_id).await?;
            if line.revision != expected.revision {
                return Err(AppError::Conflict(format!(
                    "suggestion conflicts with line {}",
                    expected.line_id
                )));
            }
        }
        let content: Vec<String> = serde_json::from_str(&suggestion.content_json)?;
        match suggestion.kind.as_str() {
            "insert" => {
                let orders = insertion_orders_tx(
                    &mut tx,
                    document_id,
                    suggestion.anchor_line_id.as_deref(),
                    content.len(),
                )
                .await?;
                for (order, value) in orders.into_iter().zip(content.iter()) {
                    insert_line_at(&mut tx, document_id, order, value).await?;
                }
            }
            "replace" => {
                let start = suggestion.start_line_id.as_deref().ok_or_else(|| {
                    AppError::BadRequest("replace suggestion requires start line".into())
                })?;
                let end = suggestion.end_line_id.as_deref().unwrap_or(start);
                let ids = line_range_tx(&mut tx, document_id, start, end).await?;
                if ids.len() != content.len() {
                    return Err(AppError::Conflict(
                        "replacement line count no longer matches target range".into(),
                    ));
                }
                let timestamp = now();
                for (id, value) in ids.iter().zip(content.iter()) {
                    sqlx::query("UPDATE lines SET content = ?, revision = revision + 1, updated_at = ? WHERE id = ?")
                        .bind(value)
                        .bind(&timestamp)
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            "delete" => {
                let start = suggestion.start_line_id.as_deref().ok_or_else(|| {
                    AppError::BadRequest("delete suggestion requires start line".into())
                })?;
                let end = suggestion.end_line_id.as_deref().unwrap_or(start);
                let ids = line_range_tx(&mut tx, document_id, start, end).await?;
                let timestamp = now();
                for id in ids {
                    sqlx::query("UPDATE lines SET deleted = 1, revision = revision + 1, updated_at = ? WHERE id = ?")
                        .bind(&timestamp)
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            _ => return Err(AppError::BadRequest("unknown suggestion kind".into())),
        }
        touch_document_tx(&mut tx, document_id).await?;
    }
    let timestamp = now();
    let state = if accept { "accepted" } else { "rejected" };
    sqlx::query("UPDATE suggestions SET state = ?, decided_by_client_id = ?, decided_by_nickname = ?, decided_at = ? WHERE id = ?")
        .bind(state)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(&timestamp)
        .bind(suggestion_id)
        .execute(&mut *tx)
        .await?;
    audit_tx(
        &mut tx,
        document_id,
        actor,
        if accept {
            "suggestion.accepted"
        } else {
            "suggestion.rejected"
        },
        json!({ "suggestion_id": suggestion_id }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, document_id).await
}

async fn insert_line_at(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    order_index: i64,
    content: &str,
) -> Result<Line> {
    let timestamp = now();
    let line = Line {
        id: new_id(),
        document_id: document_id.to_string(),
        order_index,
        content: content.to_string(),
        revision: 1,
        deleted: false,
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
    };
    sqlx::query("INSERT INTO lines (id, document_id, order_index, content, revision, deleted, created_at, updated_at) VALUES (?, ?, ?, ?, 1, 0, ?, ?)")
        .bind(&line.id)
        .bind(&line.document_id)
        .bind(line.order_index)
        .bind(&line.content)
        .bind(&line.created_at)
        .bind(&line.updated_at)
        .execute(&mut **tx)
        .await?;
    Ok(line)
}

async fn begin_write_tx(pool: &SqlitePool) -> Result<Transaction<'_, Sqlite>> {
    Ok(pool.begin_with("BEGIN IMMEDIATE").await?)
}

async fn touch_document_tx(tx: &mut Transaction<'_, Sqlite>, document_id: &str) -> Result<()> {
    sqlx::query("UPDATE documents SET updated_at = ? WHERE id = ?")
        .bind(now())
        .bind(document_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn ensure_unlocked_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Identity,
    document_id: &str,
    line_ids: &[String],
) -> Result<()> {
    let timestamp = Utc::now().to_rfc3339();
    prune_expired_locks_tx(tx).await?;
    for line_id in line_ids {
        ensure_line_exists_tx(tx, document_id, line_id).await?;
        if let Some(lock) = sqlx::query_as::<_, LineLock>("SELECT line_id, document_id, owner_client_id, owner_nickname, expires_at FROM locks WHERE line_id = ? AND document_id = ? AND expires_at > ?")
            .bind(line_id)
            .bind(document_id)
            .bind(&timestamp)
            .fetch_optional(&mut **tx)
            .await?
            .filter(|lock| lock.owner_client_id != actor.client_id)
        {
            return Err(AppError::Conflict(format!(
                "line {} is locked by {}",
                lock.line_id, lock.owner_nickname
            )));
        }
    }
    Ok(())
}

async fn insertion_orders_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    after_line_id: Option<&str>,
    count: usize,
) -> Result<Vec<i64>> {
    let mut bounds = insertion_bounds_tx(tx, document_id, after_line_id).await?;
    if let (Some(lower), Some(upper)) = bounds
        && upper - lower <= count as i64
    {
        renumber_active_tx(tx, document_id).await?;
        bounds = insertion_bounds_tx(tx, document_id, after_line_id).await?;
    }

    let orders = match bounds {
        (Some(lower), Some(upper)) => {
            let gap = upper - lower;
            if gap <= count as i64 {
                return Err(AppError::Conflict("not enough line ordering space".into()));
            }
            let step = gap / (count as i64 + 1);
            (1..=count as i64)
                .map(|index| lower + step * index)
                .collect()
        }
        (Some(lower), None) => (1..=count as i64)
            .map(|index| lower + LINE_ORDER_STEP * index)
            .collect(),
        (None, Some(upper)) => (0..count as i64)
            .map(|index| upper - LINE_ORDER_STEP * (count as i64 - index))
            .collect(),
        (None, None) => (1..=count as i64)
            .map(|index| LINE_ORDER_STEP * index)
            .collect(),
    };
    Ok(orders)
}

async fn insertion_bounds_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    after_line_id: Option<&str>,
) -> Result<(Option<i64>, Option<i64>)> {
    if let Some(line_id) = after_line_id {
        let lower = order_for_line_tx(tx, document_id, line_id).await?;
        let upper: Option<(i64,)> = sqlx::query_as(
            "SELECT order_index FROM lines WHERE document_id = ? AND deleted = 0 AND order_index > ? ORDER BY order_index, created_at LIMIT 1",
        )
        .bind(document_id)
        .bind(lower)
        .fetch_optional(&mut **tx)
        .await?;
        Ok((Some(lower), upper.map(|row| row.0)))
    } else {
        let upper: Option<(i64,)> = sqlx::query_as(
            "SELECT order_index FROM lines WHERE document_id = ? AND deleted = 0 ORDER BY order_index, created_at LIMIT 1",
        )
        .bind(document_id)
        .fetch_optional(&mut **tx)
        .await?;
        Ok((None, upper.map(|row| row.0)))
    }
}

async fn active_lines_tx(tx: &mut Transaction<'_, Sqlite>, document_id: &str) -> Result<Vec<Line>> {
    Ok(sqlx::query_as::<_, Line>(
        "SELECT id, document_id, order_index, content, revision, deleted, created_at, updated_at FROM lines WHERE document_id = ? AND deleted = 0 ORDER BY order_index, created_at",
    )
    .bind(document_id)
    .fetch_all(&mut **tx)
    .await?)
}

async fn active_content_tx(tx: &mut Transaction<'_, Sqlite>, document_id: &str) -> Result<String> {
    Ok(active_lines_tx(tx, document_id)
        .await?
        .into_iter()
        .map(|line| line.content)
        .collect::<Vec<_>>()
        .join("\n"))
}

async fn ensure_line_exists_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    line_id: &str,
) -> Result<()> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM lines WHERE id = ? AND document_id = ? AND deleted = 0",
    )
    .bind(line_id)
    .bind(document_id)
    .fetch_one(&mut **tx)
    .await?;
    if count.0 == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

async fn get_line_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    line_id: &str,
) -> Result<Line> {
    Ok(sqlx::query_as::<_, Line>("SELECT id, document_id, order_index, content, revision, deleted, created_at, updated_at FROM lines WHERE id = ? AND document_id = ? AND deleted = 0")
        .bind(line_id)
        .bind(document_id)
        .fetch_one(&mut **tx)
        .await?)
}

async fn order_for_line_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    line_id: &str,
) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "SELECT order_index FROM lines WHERE id = ? AND document_id = ? AND deleted = 0",
    )
    .bind(line_id)
    .bind(document_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.0)
}

async fn renumber_active_tx(tx: &mut Transaction<'_, Sqlite>, document_id: &str) -> Result<()> {
    let ids: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM lines WHERE document_id = ? AND deleted = 0 ORDER BY order_index, created_at",
    )
    .bind(document_id)
    .fetch_all(&mut **tx)
    .await?;
    for (index, (id,)) in ids.into_iter().enumerate() {
        sqlx::query("UPDATE lines SET order_index = ? WHERE id = ?")
            .bind((index as i64 + 1) * LINE_ORDER_STEP)
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn line_range_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    start_line_id: &str,
    end_line_id: &str,
) -> Result<Vec<String>> {
    let start = order_for_line_tx(tx, document_id, start_line_id).await?;
    let end = order_for_line_tx(tx, document_id, end_line_id).await?;
    let (lower, upper) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM lines WHERE document_id = ? AND deleted = 0 AND order_index BETWEEN ? AND ? ORDER BY order_index")
        .bind(document_id)
        .bind(lower)
        .bind(upper)
        .fetch_all(&mut **tx)
        .await?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

async fn get_comment_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    comment_id: &str,
) -> Result<Comment> {
    Ok(sqlx::query_as::<_, Comment>("SELECT id, document_id, start_line_id, end_line_id, start_column, end_column, author_client_id, author_nickname, role_mode, body, resolved, created_at, updated_at FROM comments WHERE id = ? AND document_id = ?")
        .bind(comment_id)
        .bind(document_id)
        .fetch_one(&mut **tx)
        .await?)
}

async fn ensure_document_exists(pool: &SqlitePool, document_id: &str) -> Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM documents WHERE id = ?")
        .bind(document_id)
        .fetch_one(pool)
        .await?;
    if count.0 == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

fn push_history_category_filter(builder: &mut QueryBuilder<'_, Sqlite>, category: HistoryCategory) {
    match category {
        HistoryCategory::DocumentComment => {
            builder.push(" AND (");
            push_content_event_filter(builder);
            builder.push(" OR a.event_type LIKE 'comment.%')");
        }
        HistoryCategory::All => {}
        HistoryCategory::Content => {
            builder.push(" AND ");
            push_content_event_filter(builder);
        }
        HistoryCategory::Comment => {
            builder.push(" AND a.event_type LIKE 'comment.%'");
        }
        HistoryCategory::Suggestion => {
            builder.push(" AND a.event_type LIKE 'suggestion.%'");
        }
        HistoryCategory::System => {
            builder.push(" AND NOT (");
            push_content_event_filter(builder);
            builder.push(" OR a.event_type LIKE 'comment.%' OR a.event_type LIKE 'suggestion.%')");
        }
    }
}

fn push_content_event_filter(builder: &mut QueryBuilder<'_, Sqlite>) {
    builder.push(
        "(a.event_type IN ('document.created', 'document.content_updated') OR a.event_type LIKE 'lines.%')",
    );
}

async fn audit_event(
    pool: &SqlitePool,
    document_id: &str,
    audit_event_id: &str,
) -> Result<AuditEvent> {
    Ok(sqlx::query_as::<_, AuditEvent>(
        "SELECT a.id, a.document_id, a.actor_client_id, a.actor_nickname, a.role_mode, a.event_type, a.details_json, a.created_at, n.body AS note_body, n.updated_by_nickname AS note_updated_by_nickname, n.updated_at AS note_updated_at FROM audit_events a LEFT JOIN audit_event_notes n ON n.audit_event_id = a.id WHERE a.document_id = ? AND a.id = ?",
    )
    .bind(document_id)
    .bind(audit_event_id)
    .fetch_one(pool)
    .await?)
}

async fn version_content(
    pool: &SqlitePool,
    document_id: &str,
    audit_event_id: &str,
) -> Result<String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT content FROM document_versions WHERE document_id = ? AND audit_event_id = ?",
    )
    .bind(document_id)
    .bind(audit_event_id)
    .fetch_optional(pool)
    .await?;
    row.map(|(content,)| content)
        .ok_or_else(|| AppError::Conflict("版本数据不可用".into()))
}

async fn audit_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    actor: &Identity,
    event_type: &str,
    details: serde_json::Value,
) -> Result<()> {
    let audit_event_id = new_id();
    let timestamp = now();
    sqlx::query("INSERT INTO audit_events (id, document_id, actor_client_id, actor_nickname, role_mode, event_type, details_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&audit_event_id)
        .bind(document_id)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(actor.role_mode.as_str())
        .bind(event_type)
        .bind(serde_json::to_string(&details)?)
        .bind(&timestamp)
        .execute(&mut **tx)
        .await?;
    let content = active_content_tx(tx, document_id).await?;
    sqlx::query(
        "INSERT INTO document_versions (audit_event_id, document_id, content, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&audit_event_id)
    .bind(document_id)
    .bind(content)
    .bind(timestamp)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn prune_expired_locks_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    sqlx::query("DELETE FROM locks WHERE expires_at <= ?")
        .bind(Utc::now().to_rfc3339())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn require_writer(actor: &Identity) -> Result<()> {
    if actor.role_mode != RoleMode::Writer {
        return Err(AppError::Forbidden(
            "this operation requires writer mode".into(),
        ));
    }
    Ok(())
}

fn split_lines_preserve_trailing(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return vec![];
    }
    let mut parts: Vec<&str> = content.split('\n').collect();
    for line in &mut parts {
        if let Some(stripped) = line.strip_suffix('\r') {
            *line = stripped;
        }
    }
    if content.chars().all(|c| c == '\n' || c == '\r') {
        parts.pop();
    }
    parts
}

fn text_summary(value: &str) -> String {
    value.chars().take(120).collect()
}

fn text_len(value: &str) -> usize {
    value.chars().count()
}

fn line_count(value: &str) -> usize {
    split_lines_preserve_trailing(value).len()
}

#[allow(dead_code)]
fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .map_err(|err| AppError::BadRequest(err.to_string()))?
        .with_timezone(&Utc))
}
