use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite};

use crate::error::{AppError, Result};
use crate::models::*;

use super::audit::{audit_tx, begin_write_tx, touch_page_tx};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockInput {
    pub block_type: String,
    pub content_json: String,
    #[serde(default)]
    pub properties_json: Option<String>,
}

pub async fn get_block(pool: &SqlitePool, block_id: &str) -> Result<Block> {
    Ok(sqlx::query_as::<_, Block>(
        "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE id = ? AND deleted = 0",
    )
    .bind(block_id)
    .fetch_one(pool)
    .await?)
}

pub async fn list_children(
    pool: &SqlitePool,
    parent_block_id: Option<&str>,
    page_id: &str,
    cursor: Option<f64>,
    page_size: i64,
) -> Result<(Vec<Block>, Option<f64>, bool)> {
    if page_size < 1 {
        return Err(AppError::BadRequest("page_size must be at least 1".into()));
    }

    let take = page_size + 1; // fetch one extra to determine has_more

    let blocks = if let Some(parent_id) = parent_block_id {
        if let Some(c) = cursor {
            sqlx::query_as::<_, Block>(
                "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE page_id = ? AND deleted = 0 AND parent_id = ? AND order_index > ? ORDER BY order_index LIMIT ?",
            )
            .bind(page_id)
            .bind(parent_id)
            .bind(c)
            .bind(take)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, Block>(
                "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE page_id = ? AND deleted = 0 AND parent_id = ? ORDER BY order_index LIMIT ?",
            )
            .bind(page_id)
            .bind(parent_id)
            .bind(take)
            .fetch_all(pool)
            .await?
        }
    } else if let Some(c) = cursor {
        sqlx::query_as::<_, Block>(
            "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE page_id = ? AND deleted = 0 AND parent_id IS NULL AND order_index > ? ORDER BY order_index LIMIT ?",
        )
        .bind(page_id)
        .bind(c)
        .bind(take)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Block>(
            "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE page_id = ? AND deleted = 0 AND parent_id IS NULL ORDER BY order_index LIMIT ?",
        )
        .bind(page_id)
        .bind(take)
        .fetch_all(pool)
        .await?
    };

    let has_more = blocks.len() > page_size as usize;
    let result_blocks = if has_more {
        &blocks[..page_size as usize]
    } else {
        &blocks[..]
    };
    let next_cursor = result_blocks.last().map(|b| b.order_index);

    Ok((result_blocks.to_vec(), next_cursor, has_more))
}

pub async fn append_blocks(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    blocks: Vec<BlockInput>,
    after: Option<&str>,
) -> Result<PageSnapshot> {
    if blocks.is_empty() {
        return Err(AppError::BadRequest("at least one block is required".into()));
    }

    let mut tx = begin_write_tx(pool).await?;

    let base_order = if let Some(after_id) = after {
        let after_order = get_order_tx(&mut tx, after_id).await?;
        after_order
    } else {
        let max_val = max_order_tx(&mut tx, page_id).await?;
        max_val.unwrap_or(0.0)
    };

    // Try midpoint insertion when there's a next sibling
    let next_order = if let Some(after_id) = after {
        let after_order = get_order_tx(&mut tx, after_id).await?;
        next_order_tx(&mut tx, page_id, after_order).await?
    } else {
        None
    };

    let mut inserted = Vec::new();
    if let Some(next) = next_order {
        let gap = next - base_order;
        let step = gap / (blocks.len() as f64 + 1.0);
        for (i, input) in blocks.iter().enumerate() {
            let block = insert_block_tx(
                &mut tx,
                page_id,
                None,
                base_order + (i as f64 + 1.0) * step,
                &input.block_type,
                &input.content_json,
                input.properties_json.as_deref().unwrap_or("{}"),
            )
            .await?;
            inserted.push(block);
        }
    } else {
        for (i, input) in blocks.iter().enumerate() {
            let block = insert_block_tx(
                &mut tx,
                page_id,
                None,
                base_order + (i as f64 + 1.0) * 1000.0,
                &input.block_type,
                &input.content_json,
                input.properties_json.as_deref().unwrap_or("{}"),
            )
            .await?;
            inserted.push(block);
        }
    }

    let block_ids: Vec<String> = inserted.iter().map(|b| b.id.clone()).collect();
    let blocks_detail: Vec<_> = inserted
        .iter()
        .map(|b| {
            json!({
                "block_id": b.id,
                "block_type": b.block_type,
            })
        })
        .collect();

    touch_page_tx(&mut tx, page_id).await?;
    audit_tx(
        &mut tx,
        page_id,
        actor,
        "blocks.appended",
        json!({
            "block_ids": block_ids,
            "blocks": blocks_detail,
            "count": inserted.len(),
        }),
    )
    .await?;

    tx.commit().await?;
    super::page::snapshot(pool, page_id).await
}

pub async fn update_block(
    pool: &SqlitePool,
    actor: &Identity,
    block_id: &str,
    block_type: Option<&str>,
    content_json: Option<&str>,
    properties_json: Option<&str>,
) -> Result<PageSnapshot> {
    let mut tx = begin_write_tx(pool).await?;
    let block = get_block_tx(&mut tx, block_id).await?;
    let page_id = block.page_id.clone();
    let timestamp = now();

    let mut updated = false;
    if let Some(bt) = block_type {
        sqlx::query(
            "UPDATE blocks SET block_type = ?, revision = revision + 1, updated_at = ? WHERE id = ? AND deleted = 0",
        )
        .bind(bt)
        .bind(&timestamp)
        .bind(block_id)
        .execute(&mut *tx)
        .await?;
        updated = true;
    }
    if let Some(cj) = content_json {
        sqlx::query(
            "UPDATE blocks SET content_json = ?, revision = revision + 1, updated_at = ? WHERE id = ? AND deleted = 0",
        )
        .bind(cj)
        .bind(&timestamp)
        .bind(block_id)
        .execute(&mut *tx)
        .await?;
        updated = true;
    }
    if let Some(pj) = properties_json {
        sqlx::query(
            "UPDATE blocks SET properties_json = ?, revision = revision + 1, updated_at = ? WHERE id = ? AND deleted = 0",
        )
        .bind(pj)
        .bind(&timestamp)
        .bind(block_id)
        .execute(&mut *tx)
        .await?;
        updated = true;
    }

    if !updated {
        sqlx::query(
            "UPDATE blocks SET revision = revision + 1, updated_at = ? WHERE id = ? AND deleted = 0",
        )
        .bind(&timestamp)
        .bind(block_id)
        .execute(&mut *tx)
        .await?;
    }

    touch_page_tx(&mut tx, &page_id).await?;
    audit_tx(
        &mut tx,
        &page_id,
        actor,
        "block.updated",
        json!({
            "block_id": block_id,
            "before_block_type": block.block_type,
            "before_content_summary": text_summary(&block.content_json),
        }),
    )
    .await?;

    tx.commit().await?;
    super::page::snapshot(pool, &page_id).await
}

pub async fn delete_block(
    pool: &SqlitePool,
    actor: &Identity,
    block_id: &str,
) -> Result<PageSnapshot> {
    let mut tx = begin_write_tx(pool).await?;
    let block = get_block_tx(&mut tx, block_id).await?;
    let page_id = block.page_id.clone();
    let timestamp = now();

    let affected = sqlx::query(
        "UPDATE blocks SET deleted = 1, revision = revision + 1, updated_at = ? WHERE id = ? AND deleted = 0",
    )
    .bind(&timestamp)
    .bind(block_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if affected != 1 {
        return Err(AppError::NotFound);
    }

    touch_page_tx(&mut tx, &page_id).await?;
    audit_tx(
        &mut tx,
        &page_id,
        actor,
        "block.deleted",
        json!({
            "block_id": block_id,
            "block_type": block.block_type,
        }),
    )
    .await?;

    tx.commit().await?;
    super::page::snapshot(pool, &page_id).await
}

pub async fn insert_block_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    parent_id: Option<&str>,
    order_index: f64,
    block_type: &str,
    content_json: &str,
    properties_json: &str,
) -> Result<Block> {
    let timestamp = now();
    let block = Block {
        id: new_id(),
        page_id: page_id.to_string(),
        parent_id: parent_id.map(|s| s.to_string()),
        order_index,
        block_type: block_type.to_string(),
        content_json: content_json.to_string(),
        properties_json: properties_json.to_string(),
        revision: 1,
        deleted: false,
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
    };
    sqlx::query(
        "INSERT INTO blocks (id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 1, 0, ?, ?)",
    )
    .bind(&block.id)
    .bind(&block.page_id)
    .bind(&block.parent_id)
    .bind(block.order_index)
    .bind(&block.block_type)
    .bind(&block.content_json)
    .bind(&block.properties_json)
    .bind(&block.created_at)
    .bind(&block.updated_at)
    .execute(&mut **tx)
    .await?;
    Ok(block)
}

// ── helpers used by other db modules ──

pub(crate) async fn ensure_block_exists_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    block_id: &str,
) -> Result<()> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM blocks WHERE id = ? AND page_id = ? AND deleted = 0",
    )
    .bind(block_id)
    .bind(page_id)
    .fetch_one(&mut **tx)
    .await?;
    if count.0 == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub(crate) async fn get_block_tx(
    tx: &mut Transaction<'_, Sqlite>,
    block_id: &str,
) -> Result<Block> {
    Ok(sqlx::query_as::<_, Block>(
        "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE id = ? AND deleted = 0",
    )
    .bind(block_id)
    .fetch_one(&mut **tx)
    .await?)
}

// ── private helpers ──

async fn get_order_tx(
    tx: &mut Transaction<'_, Sqlite>,
    block_id: &str,
) -> Result<f64> {
    let row: (f64,) = sqlx::query_as(
        "SELECT order_index FROM blocks WHERE id = ? AND deleted = 0",
    )
    .bind(block_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.0)
}

async fn next_order_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    after_order: f64,
) -> Result<Option<f64>> {
    let row: Option<(f64,)> = sqlx::query_as(
        "SELECT order_index FROM blocks WHERE page_id = ? AND deleted = 0 AND parent_id IS NULL AND order_index > ? ORDER BY order_index LIMIT 1",
    )
    .bind(page_id)
    .bind(after_order)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|r| r.0))
}

async fn max_order_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<Option<f64>> {
    let row: Option<(f64,)> = sqlx::query_as(
        "SELECT MAX(order_index) FROM blocks WHERE page_id = ? AND deleted = 0 AND parent_id IS NULL",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|r| r.0))
}

fn text_summary(value: &str) -> String {
    value.chars().take(120).collect()
}
