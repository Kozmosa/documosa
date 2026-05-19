use serde_json::json;
use sqlx::SqlitePool;

use crate::error::{AppError, Result};
use crate::models::*;

#[allow(clippy::too_many_arguments)]
pub async fn create_suggestion(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    kind: String,
    target_block_id: Option<String>,
    content: Vec<String>,
) -> Result<PageSnapshot> {
    if !matches!(kind.as_str(), "insert" | "replace" | "delete") {
        return Err(AppError::BadRequest(
            "suggestion kind must be insert, replace, or delete".into(),
        ));
    }
    let mut tx = super::audit::begin_write_tx(pool).await?;
    let mut base_revisions = Vec::new();
    for id in target_block_id.iter() {
        let block = super::block::get_block_tx(&mut tx, id).await?;
        base_revisions.push(BaseRevision {
            block_id: block.id,
            revision: block.revision,
        });
    }
    let id = new_id();
    let timestamp = now();
    sqlx::query("INSERT INTO suggestions (id, page_id, kind, target_block_id, parent_id, content_json, base_revisions_json, state, author_client_id, author_nickname, role_mode, created_at) VALUES (?, ?, ?, ?, NULL, ?, ?, 'open', ?, ?, ?, ?)")
        .bind(&id)
        .bind(page_id)
        .bind(&kind)
        .bind(&target_block_id)
        .bind(serde_json::to_string(&content)?)
        .bind(serde_json::to_string(&base_revisions)?)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(actor.role_mode.as_str())
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
    super::audit::audit_tx(
        &mut tx,
        page_id,
        actor,
        "suggestion.created",
        json!({ "suggestion_id": id, "kind": kind }),
    )
    .await?;
    tx.commit().await?;
    super::page::snapshot(pool, page_id).await
}

#[allow(clippy::too_many_arguments)]
pub async fn decide_suggestion(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    suggestion_id: &str,
    accept: bool,
) -> Result<PageSnapshot> {
    super::require_permission(actor, if accept { "accept_suggestion" } else { "reject_suggestion" })?;
    let mut tx = super::audit::begin_write_tx(pool).await?;
    let suggestion = sqlx::query_as::<_, Suggestion>(
        "SELECT id, page_id, kind, target_block_id, parent_id, content_json, base_revisions_json, state, author_client_id, author_nickname, role_mode, created_at, decided_by_client_id, decided_by_nickname, decided_at FROM suggestions WHERE id = ? AND page_id = ?",
    )
    .bind(suggestion_id)
    .bind(page_id)
    .fetch_one(&mut *tx)
    .await?;
    if suggestion.state != "open" {
        return Err(AppError::Conflict("suggestion is already decided".into()));
    }
    if accept {
        let revisions: Vec<BaseRevision> = serde_json::from_str(&suggestion.base_revisions_json)?;
        for expected in revisions {
            let block = super::block::get_block_tx(&mut tx, &expected.block_id).await?;
            if block.revision != expected.revision {
                return Err(AppError::Conflict(format!(
                    "suggestion conflicts with block {}",
                    expected.block_id
                )));
            }
        }
        let content: Vec<String> = serde_json::from_str(&suggestion.content_json)?;
        match suggestion.kind.as_str() {
            "insert" => {
                let target = suggestion.target_block_id.as_deref();
                let base_order = if let Some(tid) = target {
                    let row: (f64,) = sqlx::query_as(
                        "SELECT order_index FROM blocks WHERE id = ? AND deleted = 0",
                    )
                    .bind(tid)
                    .fetch_one(&mut *tx)
                    .await?;
                    row.0
                } else {
                    0.0
                };
                for (i, value) in content.iter().enumerate() {
                    super::block::insert_block_tx(
                        &mut tx,
                        page_id,
                        None,
                        base_order + (i as f64 + 1.0) * 1000.0,
                        "text",
                        &serde_json::to_string(&[serde_json::json!({
                            "type": "text",
                            "text": { "content": value },
                            "plain_text": value,
                        })])?,
                        "{}",
                    )
                    .await?;
                }
            }
            "replace" => {
                let target = suggestion.target_block_id.as_deref().ok_or_else(|| {
                    AppError::BadRequest("replace suggestion requires target block".into())
                })?;
                let new_content_json = serde_json::to_string(&content.iter().map(|c| {
                    serde_json::json!({
                        "type": "text",
                        "text": { "content": c },
                        "plain_text": c,
                    })
                }).collect::<Vec<_>>())?;
                let timestamp = now();
                sqlx::query(
                    "UPDATE blocks SET content_json = ?, revision = revision + 1, updated_at = ? WHERE id = ? AND deleted = 0",
                )
                .bind(&new_content_json)
                .bind(&timestamp)
                .bind(target)
                .execute(&mut *tx)
                .await?;
            }
            "delete" => {
                let target = suggestion.target_block_id.as_deref().ok_or_else(|| {
                    AppError::BadRequest("delete suggestion requires target block".into())
                })?;
                let timestamp = now();
                sqlx::query(
                    "UPDATE blocks SET deleted = 1, revision = revision + 1, updated_at = ? WHERE id = ? AND deleted = 0",
                )
                .bind(&timestamp)
                .bind(target)
                .execute(&mut *tx)
                .await?;
            }
            _ => return Err(AppError::BadRequest("unknown suggestion kind".into())),
        }
        super::audit::touch_page_tx(&mut tx, page_id).await?;
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
    super::audit::audit_tx(
        &mut tx,
        page_id,
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
    super::page::snapshot(pool, page_id).await
}
