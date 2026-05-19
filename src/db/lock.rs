use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite, QueryBuilder};

use crate::error::{AppError, Result};
use crate::models::*;

pub async fn heartbeat_locks(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    block_ids: Vec<String>,
) -> Result<Vec<BlockLock>> {
    if block_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut tx = super::audit::begin_write_tx(pool).await?;
    ensure_unlocked_tx(&mut tx, actor, page_id, &block_ids).await?;
    let expires_at = (Utc::now() + Duration::seconds(60)).to_rfc3339();
    for block_id in &block_ids {
        sqlx::query(
            "INSERT INTO block_locks (block_id, page_id, owner_client_id, owner_nickname, expires_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(block_id) DO UPDATE SET owner_client_id = excluded.owner_client_id, owner_nickname = excluded.owner_nickname, expires_at = excluded.expires_at",
        )
        .bind(block_id)
        .bind(page_id)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(&expires_at)
        .execute(&mut *tx)
        .await?;
    }
    super::audit::audit_tx(
        &mut tx,
        page_id,
        actor,
        "locks.heartbeat",
        json!({ "block_ids": block_ids }),
    )
    .await?;
    tx.commit().await?;
    locks(pool, page_id).await
}

pub async fn release_locks(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    block_ids: Vec<String>,
) -> Result<Vec<BlockLock>> {
    let mut tx = super::audit::begin_write_tx(pool).await?;
    super::audit::prune_expired_locks_tx(&mut tx).await?;
    let mut builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("DELETE FROM block_locks WHERE page_id = ");
    builder.push_bind(page_id);
    builder.push(" AND owner_client_id = ");
    builder.push_bind(&actor.client_id);
    if !block_ids.is_empty() {
        builder.push(" AND block_id IN (");
        let mut separated = builder.separated(", ");
        for id in block_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
    }
    builder.build().execute(&mut *tx).await?;
    tx.commit().await?;
    locks(pool, page_id).await
}

pub async fn locks(pool: &SqlitePool, page_id: &str) -> Result<Vec<BlockLock>> {
    let timestamp = now();
    Ok(sqlx::query_as::<_, BlockLock>(
        "SELECT block_id, page_id, owner_client_id, owner_nickname, expires_at FROM block_locks WHERE page_id = ? AND expires_at > ? ORDER BY expires_at",
    )
    .bind(page_id)
    .bind(&timestamp)
    .fetch_all(pool)
    .await?)
}

pub(super) async fn ensure_unlocked_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Identity,
    page_id: &str,
    block_ids: &[String],
) -> Result<()> {
    let timestamp = Utc::now().to_rfc3339();
    super::audit::prune_expired_locks_tx(tx).await?;
    for block_id in block_ids {
        super::block::ensure_block_exists_tx(tx, page_id, block_id).await?;
        if let Some(lock) = sqlx::query_as::<_, BlockLock>("SELECT block_id, page_id, owner_client_id, owner_nickname, expires_at FROM block_locks WHERE block_id = ? AND page_id = ? AND expires_at > ?")
            .bind(block_id)
            .bind(page_id)
            .bind(&timestamp)
            .fetch_optional(&mut **tx)
            .await?
            .filter(|lock| lock.owner_client_id != actor.client_id)
        {
            return Err(AppError::Conflict(format!(
                "block {} is locked by {}",
                lock.block_id, lock.owner_nickname
            )));
        }
    }
    Ok(())
}
