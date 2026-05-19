use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite};

use crate::error::{AppError, Result};
use crate::models::*;

pub struct CommentDraft {
    pub target_block_id: String,
    pub start_column: Option<i64>,
    pub end_column: Option<i64>,
    pub body: String,
}

async fn get_comment_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    comment_id: &str,
) -> Result<Comment> {
    Ok(sqlx::query_as::<_, Comment>("SELECT id, page_id, target_block_id, start_column, end_column, author_client_id, author_nickname, role_mode, body, resolved, created_at, updated_at FROM comments WHERE id = ? AND page_id = ?")
        .bind(comment_id)
        .bind(page_id)
        .fetch_one(&mut **tx)
        .await?)
}

pub async fn create_comment(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    draft: CommentDraft,
) -> Result<PageSnapshot> {
    if draft.body.trim().is_empty() {
        return Err(AppError::BadRequest("comment body is required".into()));
    }
    let mut tx = super::audit::begin_write_tx(pool).await?;
    super::block::ensure_block_exists_tx(&mut tx, page_id, &draft.target_block_id).await?;
    if draft.start_column.is_some_and(|column| column < 0)
        || draft.end_column.is_some_and(|column| column < 0)
    {
        return Err(AppError::BadRequest(
            "comment columns must be non-negative".into(),
        ));
    }
    let timestamp = now();
    let id = new_id();
    sqlx::query("INSERT INTO comments (id, page_id, target_block_id, start_column, end_column, author_client_id, author_nickname, role_mode, body, resolved, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)")
        .bind(&id)
        .bind(page_id)
        .bind(&draft.target_block_id)
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
        page_id,
        actor,
        "comment.created",
        json!({
            "comment_id": id,
            "target_block_id": draft.target_block_id,
            "start_column": draft.start_column,
            "end_column": draft.end_column,
            "body": draft.body,
        }),
    )
    .await?;
    tx.commit().await?;
    super::page::snapshot(pool, page_id).await
}

pub async fn update_comment(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    comment_id: &str,
    body: String,
) -> Result<PageSnapshot> {
    let mut tx = super::audit::begin_write_tx(pool).await?;
    let existing = get_comment_tx(&mut tx, page_id, comment_id).await?;
    if actor.role_mode == RoleMode::Reviewer && existing.author_client_id != actor.client_id {
        return Err(AppError::Forbidden(
            "reviewers may update only their own comments".into(),
        ));
    }
    let timestamp = now();
    sqlx::query("UPDATE comments SET body = ?, updated_at = ? WHERE id = ? AND page_id = ?")
        .bind(&body)
        .bind(&timestamp)
        .bind(comment_id)
        .bind(page_id)
        .execute(&mut *tx)
        .await?;
    super::audit::audit_tx(
        &mut tx,
        page_id,
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
    super::page::snapshot(pool, page_id).await
}

pub async fn delete_comment(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    comment_id: &str,
) -> Result<PageSnapshot> {
    let mut tx = super::audit::begin_write_tx(pool).await?;
    let existing = get_comment_tx(&mut tx, page_id, comment_id).await?;
    if actor.role_mode == RoleMode::Reviewer && existing.author_client_id != actor.client_id {
        return Err(AppError::Forbidden(
            "reviewers may delete only their own comments".into(),
        ));
    }
    sqlx::query("DELETE FROM comments WHERE id = ? AND page_id = ?")
        .bind(comment_id)
        .bind(page_id)
        .execute(&mut *tx)
        .await?;
    super::audit::audit_tx(
        &mut tx,
        page_id,
        actor,
        "comment.deleted",
        json!({
            "comment_id": comment_id,
            "body": existing.body,
        }),
    )
    .await?;
    tx.commit().await?;
    super::page::snapshot(pool, page_id).await
}

pub async fn reply_comment(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    comment_id: &str,
    body: String,
) -> Result<PageSnapshot> {
    let mut tx = super::audit::begin_write_tx(pool).await?;
    let _comment = get_comment_tx(&mut tx, page_id, comment_id).await?;
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
        page_id,
        actor,
        "comment.replied",
        json!({
            "comment_id": comment_id,
            "reply_id": id,
            "body": body,
        }),
    )
    .await?;
    tx.commit().await?;
    super::page::snapshot(pool, page_id).await
}

pub async fn resolve_comment(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    comment_id: &str,
) -> Result<PageSnapshot> {
    super::require_permission(actor, "resolve_comment")?;
    let mut tx = super::audit::begin_write_tx(pool).await?;
    let existing = get_comment_tx(&mut tx, page_id, comment_id).await?;
    let timestamp = now();
    let affected = sqlx::query(
        "UPDATE comments SET resolved = 1, updated_at = ? WHERE id = ? AND page_id = ?",
    )
    .bind(&timestamp)
    .bind(comment_id)
    .bind(page_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected != 1 {
        return Err(AppError::NotFound);
    }
    super::audit::audit_tx(
        &mut tx,
        page_id,
        actor,
        "comment.resolved",
        json!({
            "comment_id": comment_id,
            "body": existing.body,
        }),
    )
    .await?;
    tx.commit().await?;
    super::page::snapshot(pool, page_id).await
}
