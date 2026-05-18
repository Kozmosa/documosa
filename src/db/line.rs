use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite};

use crate::error::{AppError, Result};
use crate::models::*;

use super::text::*;
use super::LINE_ORDER_STEP;

pub async fn update_content(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    content: String,
    base_revisions: Vec<BaseRevision>,
) -> Result<DocumentSnapshot> {
    super::require_permission(actor, "replace_lines")?;
    let mut tx = super::audit::begin_write_tx(pool).await?;
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
    super::lock::ensure_unlocked_tx(&mut tx, actor, document_id, &touched_ids).await?;

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
    super::audit::touch_document_tx(&mut tx, document_id).await?;
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}

pub async fn insert_lines(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    after_line_id: Option<String>,
    content: Vec<String>,
) -> Result<DocumentSnapshot> {
    super::require_permission(actor, "insert_lines")?;
    if content.is_empty() {
        return Err(AppError::BadRequest("at least one line is required".into()));
    }
    let mut tx = super::audit::begin_write_tx(pool).await?;
    if let Some(anchor) = &after_line_id {
        super::lock::ensure_unlocked_tx(&mut tx, actor, document_id, std::slice::from_ref(anchor))
            .await?;
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
    super::audit::touch_document_tx(&mut tx, document_id).await?;
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}

pub async fn replace_lines(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    line_ids: Vec<String>,
    content: Vec<String>,
) -> Result<DocumentSnapshot> {
    super::require_permission(actor, "replace_lines")?;
    if line_ids.is_empty() || line_ids.len() != content.len() {
        return Err(AppError::BadRequest(
            "line_ids and content must be non-empty and equal length".into(),
        ));
    }
    let mut targets = line_ids.clone();
    targets.sort();
    let mut tx = super::audit::begin_write_tx(pool).await?;
    super::lock::ensure_unlocked_tx(&mut tx, actor, document_id, &targets).await?;
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
    super::audit::touch_document_tx(&mut tx, document_id).await?;
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}

pub async fn delete_lines(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    line_ids: Vec<String>,
) -> Result<DocumentSnapshot> {
    super::require_permission(actor, "delete_lines")?;
    if line_ids.is_empty() {
        return Err(AppError::BadRequest("line_ids are required".into()));
    }
    let mut targets = line_ids.clone();
    targets.sort();
    let mut tx = super::audit::begin_write_tx(pool).await?;
    super::lock::ensure_unlocked_tx(&mut tx, actor, document_id, &targets).await?;
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
    super::audit::touch_document_tx(&mut tx, document_id).await?;
    super::audit::audit_tx(
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
    super::document::snapshot(pool, document_id).await
}

pub(crate) async fn insert_line_at(
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

pub(crate) async fn active_lines_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
) -> Result<Vec<Line>> {
    Ok(sqlx::query_as::<_, Line>(
        "SELECT id, document_id, order_index, content, revision, deleted, created_at, updated_at FROM lines WHERE document_id = ? AND deleted = 0 ORDER BY order_index, created_at",
    )
    .bind(document_id)
    .fetch_all(&mut **tx)
    .await?)
}

pub(crate) async fn ensure_line_exists_tx(
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

pub(crate) async fn get_line_tx(
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

async fn renumber_active_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
) -> Result<()> {
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

pub(super) async fn insertion_orders_tx(
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

pub(super) async fn line_range_tx(
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

