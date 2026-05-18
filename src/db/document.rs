use serde_json::json;
use sqlx::SqlitePool;

use crate::error::{AppError, Result};
use crate::models::*;

use super::text::*;

pub async fn create_document(
    pool: &SqlitePool,
    actor: &Identity,
    title: String,
    content: String,
) -> Result<DocumentSnapshot> {
    let mut tx = super::begin_write_tx(pool).await?;
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
        super::insert_line_at(
            &mut tx,
            &document.id,
            (index as i64 + 1) * super::LINE_ORDER_STEP,
            line,
        )
        .await?;
    }
    super::audit_tx(
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
    let mut tx = super::begin_write_tx(pool).await?;
    let timestamp = now();
    sqlx::query("UPDATE documents SET title = ?, updated_at = ? WHERE id = ?")
        .bind(title)
        .bind(&timestamp)
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    super::audit_tx(
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

pub(super) async fn ensure_document_exists(
    pool: &SqlitePool,
    document_id: &str,
) -> Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM documents WHERE id = ?")
        .bind(document_id)
        .fetch_one(pool)
        .await?;
    if count.0 == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}
