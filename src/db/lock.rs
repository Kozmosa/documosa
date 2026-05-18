use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite, QueryBuilder};

use crate::error::{AppError, Result};
use crate::models::*;

pub async fn heartbeat_locks(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    line_ids: Vec<String>,
) -> Result<Vec<LineLock>> {
    if line_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut tx = super::audit::begin_write_tx(pool).await?;
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
    super::audit::audit_tx(
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
    let mut tx = super::audit::begin_write_tx(pool).await?;
    super::audit::prune_expired_locks_tx(&mut tx).await?;
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

pub(super) async fn ensure_unlocked_tx(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Identity,
    document_id: &str,
    line_ids: &[String],
) -> Result<()> {
    let timestamp = Utc::now().to_rfc3339();
    super::audit::prune_expired_locks_tx(tx).await?;
    for line_id in line_ids {
        super::line::ensure_line_exists_tx(tx, document_id, line_id).await?;
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
