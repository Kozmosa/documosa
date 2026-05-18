use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite};

use crate::error::{AppError, Result};
use crate::models::*;

pub struct CommentDraft {
    pub start_line_id: String,
    pub end_line_id: String,
    pub start_column: Option<i64>,
    pub end_column: Option<i64>,
    pub body: String,
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

pub async fn create_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    draft: CommentDraft,
) -> Result<DocumentSnapshot> {
    if draft.body.trim().is_empty() {
        return Err(AppError::BadRequest("comment body is required".into()));
    }
    let mut tx = super::audit::begin_write_tx(pool).await?;
    super::line::ensure_line_exists_tx(&mut tx, document_id, &draft.start_line_id).await?;
    super::line::ensure_line_exists_tx(&mut tx, document_id, &draft.end_line_id).await?;
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
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}

pub async fn update_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    comment_id: &str,
    body: String,
) -> Result<DocumentSnapshot> {
    let mut tx = super::audit::begin_write_tx(pool).await?;
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
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}

pub async fn delete_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    comment_id: &str,
) -> Result<DocumentSnapshot> {
    let mut tx = super::audit::begin_write_tx(pool).await?;
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
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}

pub async fn reply_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    comment_id: &str,
    body: String,
) -> Result<DocumentSnapshot> {
    let mut tx = super::audit::begin_write_tx(pool).await?;
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
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}

pub async fn resolve_comment(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    comment_id: &str,
) -> Result<DocumentSnapshot> {
    super::require_permission(actor, "resolve_comment")?;
    let mut tx = super::audit::begin_write_tx(pool).await?;
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
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}
