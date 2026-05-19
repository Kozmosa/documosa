use serde_json::json;
use sqlx::SqlitePool;

use crate::error::{AppError, Result};
use crate::models::*;

use super::audit::{audit_tx, begin_write_tx};

pub async fn create_page(
    pool: &SqlitePool,
    actor: &Identity,
    title: String,
    blocks_json: String,
) -> Result<PageSnapshot> {
    let mut tx = begin_write_tx(pool).await?;
    let timestamp = now();
    let page = Page {
        id: new_id(),
        title,
        properties_json: "{}".to_string(),
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
    };
    sqlx::query(
        "INSERT INTO pages (id, title, properties_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&page.id)
    .bind(&page.title)
    .bind(&page.properties_json)
    .bind(&page.created_at)
    .bind(&page.updated_at)
    .execute(&mut *tx)
    .await?;

    if !blocks_json.is_empty() {
        let blocks_input: Vec<super::block::BlockInput> =
            serde_json::from_str(&blocks_json)?;
        for (index, input) in blocks_input.iter().enumerate() {
            let order_index = (index as f64 + 1.0) * 1000.0;
            let props = input
                .properties_json
                .clone()
                .unwrap_or_else(|| "{}".to_string());
            super::block::insert_block_tx(
                &mut tx,
                &page.id,
                None,
                order_index,
                &input.block_type,
                &input.content_json,
                &props,
            )
            .await?;
        }
    }

    audit_tx(
        &mut tx,
        &page.id,
        actor,
        "page.created",
        json!({
            "title": page.title,
        }),
    )
    .await?;
    tx.commit().await?;
    snapshot(pool, &page.id).await
}

pub async fn get_page(pool: &SqlitePool, page_id: &str) -> Result<Page> {
    Ok(sqlx::query_as::<_, Page>(
        "SELECT id, title, properties_json, created_at, updated_at FROM pages WHERE id = ?",
    )
    .bind(page_id)
    .fetch_one(pool)
    .await?)
}

pub async fn list_pages(pool: &SqlitePool) -> Result<Vec<Page>> {
    Ok(sqlx::query_as::<_, Page>(
        "SELECT id, title, properties_json, created_at, updated_at FROM pages ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn update_page_title(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    title: &str,
) -> Result<()> {
    let mut tx = begin_write_tx(pool).await?;
    let timestamp = now();
    sqlx::query("UPDATE pages SET title = ?, updated_at = ? WHERE id = ?")
        .bind(title)
        .bind(&timestamp)
        .bind(page_id)
        .execute(&mut *tx)
        .await?;
    audit_tx(
        &mut tx,
        page_id,
        actor,
        "page.title_updated",
        json!({ "title": title }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn snapshot(pool: &SqlitePool, page_id: &str) -> Result<PageSnapshot> {
    let timestamp = now();
    let page = sqlx::query_as::<_, Page>(
        "SELECT id, title, properties_json, created_at, updated_at FROM pages WHERE id = ?",
    )
    .bind(page_id)
    .fetch_one(pool)
    .await?;

    let blocks = sqlx::query_as::<_, Block>(
        "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE page_id = ? AND deleted = 0 ORDER BY order_index",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await?;

    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, page_id, target_block_id, start_column, end_column, author_client_id, author_nickname, role_mode, body, resolved, created_at, updated_at FROM comments WHERE page_id = ? ORDER BY created_at",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await?;

    let replies = sqlx::query_as::<_, CommentReply>(
        "SELECT r.id, r.comment_id, r.author_client_id, r.author_nickname, r.role_mode, r.body, r.created_at FROM comment_replies r JOIN comments c ON c.id = r.comment_id WHERE c.page_id = ? ORDER BY r.created_at",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await?;

    let suggestions = sqlx::query_as::<_, Suggestion>(
        "SELECT id, page_id, kind, target_block_id, parent_id, content_json, base_revisions_json, state, author_client_id, author_nickname, role_mode, created_at, decided_by_client_id, decided_by_nickname, decided_at FROM suggestions WHERE page_id = ? ORDER BY created_at",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await?;

    let locks = sqlx::query_as::<_, BlockLock>(
        "SELECT block_id, page_id, owner_client_id, owner_nickname, expires_at FROM block_locks WHERE page_id = ? AND expires_at > ? ORDER BY expires_at",
    )
    .bind(page_id)
    .bind(&timestamp)
    .fetch_all(pool)
    .await?;

    let audit_events = sqlx::query_as::<_, AuditEvent>(
        "SELECT a.id, a.page_id, a.actor_client_id, a.actor_nickname, a.role_mode, a.event_type, a.details_json, a.created_at, n.body AS note_body, n.updated_by_nickname AS note_updated_by_nickname, n.updated_at AS note_updated_at FROM audit_events a LEFT JOIN audit_event_notes n ON n.audit_event_id = a.id WHERE a.page_id = ? ORDER BY a.created_at DESC LIMIT 200",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await?;

    Ok(PageSnapshot {
        page,
        blocks,
        comments,
        replies,
        suggestions,
        locks,
        audit_events,
    })
}

pub async fn export_markdown(pool: &SqlitePool, page_id: &str) -> Result<String> {
    let page = sqlx::query_as::<_, Page>(
        "SELECT id, title, properties_json, created_at, updated_at FROM pages WHERE id = ?",
    )
    .bind(page_id)
    .fetch_one(pool)
    .await?;

    let blocks = sqlx::query_as::<_, Block>(
        "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE page_id = ? AND deleted = 0 ORDER BY order_index",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await?;

    let mut markdown = format!("# {}\n\n", page.title);

    for block in &blocks {
        let tokens: Vec<serde_json::Value> =
            serde_json::from_str(&block.content_json).unwrap_or_default();
        let plain_text: String = tokens
            .iter()
            .filter_map(|t| t.get("plain_text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("");

        match block.block_type.as_str() {
            "heading1" => markdown.push_str(&format!("# {plain_text}\n\n")),
            "heading2" => markdown.push_str(&format!("## {plain_text}\n\n")),
            "heading3" => markdown.push_str(&format!("### {plain_text}\n\n")),
            "code" => {
                let language: String =
                    serde_json::from_str::<serde_json::Value>(&block.properties_json)
                        .ok()
                        .and_then(|p| {
                            p.get("language")
                                .and_then(|v| v.as_str().map(String::from))
                        })
                        .unwrap_or_default();
                markdown.push_str(&format!("```{language}\n{plain_text}\n```\n\n"));
            }
            "bulleted_list_item" => markdown.push_str(&format!("- {plain_text}\n")),
            "numbered_list_item" => {
                markdown.push_str(&format!("1. {plain_text}\n"))
            }
            "to_do" => {
                let checked: bool =
                    serde_json::from_str::<serde_json::Value>(&block.properties_json)
                        .ok()
                        .and_then(|p| p.get("checked").and_then(|v| v.as_bool()))
                        .unwrap_or(false);
                let checkbox = if checked { "[x]" } else { "[ ]" };
                markdown.push_str(&format!("- {checkbox} {plain_text}\n"));
            }
            "quote" => markdown.push_str(&format!("> {plain_text}\n\n")),
            "divider" => markdown.push_str("---\n\n"),
            _ => markdown.push_str(&format!("{plain_text}\n\n")),
        }
    }

    Ok(markdown)
}

pub(super) async fn ensure_page_exists(pool: &SqlitePool, page_id: &str) -> Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pages WHERE id = ?")
        .bind(page_id)
        .fetch_one(pool)
        .await?;
    if count.0 == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}
