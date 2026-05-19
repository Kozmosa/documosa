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
    sqlx::query_as::<_, Block>(
        "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE id = ? AND deleted = 0",
    )
    .bind(block_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
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

    let mut blocks = if let Some(parent_id) = parent_block_id {
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

    for block in &mut blocks {
        block.object = "block".into();
    }

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
) -> Result<Vec<Block>> {
    if blocks.is_empty() {
        return Err(AppError::BadRequest("at least one block is required".into()));
    }

    for input in &blocks {
        super::validate::validate_block_input(input)?;
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

    // Check if renumbering needed (gap too small for insertion)
    let needs_renumber = match next_order {
        Some(next) => (next - base_order) <= blocks.len() as f64,
        None => false,
    };
    if needs_renumber {
        renumber_page_blocks_tx(&mut tx, page_id).await?;
    }

    // Recompute positions if renumbered
    let (final_base, final_next) = if needs_renumber {
        let bo = if let Some(after_id) = after {
            get_order_tx(&mut tx, after_id).await?
        } else { 0.0 };
        let no = if let Some(after_id) = after {
            let after_order = get_order_tx(&mut tx, after_id).await?;
            next_order_tx(&mut tx, page_id, after_order).await?
        } else { None };
        (bo, no)
    } else {
        (base_order, next_order)
    };

    // Single insertion loop
    let mut inserted = Vec::new();
    if let Some(next) = final_next {
        let gap = next - final_base;
        let step = gap / (blocks.len() as f64 + 1.0);
        for (i, input) in blocks.iter().enumerate() {
            let block = insert_block_tx(
                &mut tx,
                page_id,
                None,
                final_base + (i as f64 + 1.0) * step,
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
                final_base + (i as f64 + 1.0) * 1000.0,
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
    Ok(inserted)
}

pub async fn update_block(
    pool: &SqlitePool,
    actor: &Identity,
    block_id: &str,
    block_type: Option<&str>,
    content_json: Option<&str>,
    properties_json: Option<&str>,
) -> Result<Block> {
    let mut tx = begin_write_tx(pool).await?;
    let block = get_block_tx(&mut tx, block_id).await?;
    let page_id = block.page_id.clone();

    {
        let bid = block_id.to_string();
        super::lock::ensure_unlocked_tx(&mut tx, actor, &page_id, std::slice::from_ref(&bid)).await?;
    }

    // Validate when content_json or block_type is being changed
    if content_json.is_some() || block_type.is_some() {
        let check_input = super::block::BlockInput {
            block_type: block_type.unwrap_or(&block.block_type).to_string(),
            content_json: content_json.unwrap_or(&block.content_json).to_string(),
            properties_json: properties_json.map(|s| s.to_string()),
        };
        super::validate::validate_block_input(&check_input)?;
    }

    let timestamp = now();

    let mut qb = sqlx::QueryBuilder::new("UPDATE blocks SET ");
    let mut separated = qb.separated(", ");
    if let Some(bt) = block_type {
        separated.push("block_type = ").push_bind_unseparated(bt);
    }
    if let Some(cj) = content_json {
        separated.push("content_json = ").push_bind_unseparated(cj);
    }
    if let Some(pj) = properties_json {
        separated.push("properties_json = ").push_bind_unseparated(pj);
    }
    separated.push("revision = revision + 1");
    separated.push("updated_at = ").push_bind_unseparated(&timestamp);
    qb.push(" WHERE id = ").push_bind(block_id).push(" AND deleted = 0");
    qb.build().execute(&mut *tx).await?;

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
    get_block(pool, block_id).await
}

pub async fn delete_block(
    pool: &SqlitePool,
    actor: &Identity,
    block_id: &str,
) -> Result<Block> {
    let mut tx = begin_write_tx(pool).await?;
    let block = get_block_tx(&mut tx, block_id).await?;
    let page_id = block.page_id.clone();

    {
        let bid = block_id.to_string();
        super::lock::ensure_unlocked_tx(&mut tx, actor, &page_id, std::slice::from_ref(&bid)).await?;
    }

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

    // Cascade soft-delete all descendants
    sqlx::query(
        "WITH RECURSIVE descendants(id) AS ( \
           SELECT id FROM blocks WHERE parent_id = ? AND deleted = 0 \
           UNION ALL \
           SELECT blocks.id FROM blocks JOIN descendants ON blocks.parent_id = descendants.id \
           WHERE blocks.deleted = 0 \
         ) \
         UPDATE blocks SET deleted = 1, revision = revision + 1, updated_at = ? \
         WHERE id IN (SELECT id FROM descendants)"
    )
    .bind(block_id)
    .bind(&timestamp)
    .execute(&mut *tx)
    .await?;

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
    Ok(block)
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
        object: "block".into(),
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
    sqlx::query_as::<_, Block>(
        "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE id = ? AND deleted = 0",
    )
    .bind(block_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)
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
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)?;
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

async fn renumber_page_blocks_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<()> {
    let ids: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM blocks WHERE page_id = ? AND deleted = 0 AND parent_id IS NULL ORDER BY order_index"
    )
    .bind(page_id)
    .fetch_all(&mut **tx)
    .await?;
    for (i, (id,)) in ids.into_iter().enumerate() {
        sqlx::query("UPDATE blocks SET order_index = ? WHERE id = ?")
            .bind((i as f64 + 1.0) * 1000.0)
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
