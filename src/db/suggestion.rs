use serde_json::json;
use sqlx::SqlitePool;

use crate::error::{AppError, Result};
use crate::models::*;

#[allow(clippy::too_many_arguments)]
pub async fn create_suggestion(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    kind: String,
    anchor_line_id: Option<String>,
    start_line_id: Option<String>,
    end_line_id: Option<String>,
    content: Vec<String>,
) -> Result<DocumentSnapshot> {
    if !matches!(kind.as_str(), "insert" | "replace" | "delete") {
        return Err(AppError::BadRequest(
            "suggestion kind must be insert, replace, or delete".into(),
        ));
    }
    let mut tx = super::audit::begin_write_tx(pool).await?;
    let mut base_revisions = Vec::new();
    for id in [&anchor_line_id, &start_line_id, &end_line_id]
        .into_iter()
        .flatten()
    {
        let line = super::line::get_line_tx(&mut tx, document_id, id).await?;
        base_revisions.push(BaseRevision {
            line_id: line.id,
            revision: line.revision,
        });
    }
    let id = new_id();
    let timestamp = now();
    sqlx::query("INSERT INTO suggestions (id, document_id, kind, anchor_line_id, start_line_id, end_line_id, content_json, base_revisions_json, state, author_client_id, author_nickname, role_mode, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'open', ?, ?, ?, ?)")
        .bind(&id)
        .bind(document_id)
        .bind(&kind)
        .bind(&anchor_line_id)
        .bind(&start_line_id)
        .bind(&end_line_id)
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
        document_id,
        actor,
        "suggestion.created",
        json!({ "suggestion_id": id, "kind": kind }),
    )
    .await?;
    tx.commit().await?;
    super::document::snapshot(pool, document_id).await
}

#[allow(clippy::too_many_arguments)]
pub async fn decide_suggestion(
    pool: &SqlitePool,
    actor: &Identity,
    document_id: &str,
    suggestion_id: &str,
    accept: bool,
) -> Result<DocumentSnapshot> {
    super::line::require_writer(actor)?;
    let mut tx = super::audit::begin_write_tx(pool).await?;
    let suggestion = sqlx::query_as::<_, Suggestion>(
        "SELECT id, document_id, kind, anchor_line_id, start_line_id, end_line_id, content_json, base_revisions_json, state, author_client_id, author_nickname, role_mode, created_at, decided_by_client_id, decided_by_nickname, decided_at FROM suggestions WHERE id = ? AND document_id = ?",
    )
    .bind(suggestion_id)
    .bind(document_id)
    .fetch_one(&mut *tx)
    .await?;
    if suggestion.state != "open" {
        return Err(AppError::Conflict("suggestion is already decided".into()));
    }
    if accept {
        let revisions: Vec<BaseRevision> = serde_json::from_str(&suggestion.base_revisions_json)?;
        for expected in revisions {
            let line = super::line::get_line_tx(&mut tx, document_id, &expected.line_id).await?;
            if line.revision != expected.revision {
                return Err(AppError::Conflict(format!(
                    "suggestion conflicts with line {}",
                    expected.line_id
                )));
            }
        }
        let content: Vec<String> = serde_json::from_str(&suggestion.content_json)?;
        match suggestion.kind.as_str() {
            "insert" => {
                let orders = super::line::insertion_orders_tx(
                    &mut tx,
                    document_id,
                    suggestion.anchor_line_id.as_deref(),
                    content.len(),
                )
                .await?;
                for (order, value) in orders.into_iter().zip(content.iter()) {
                    super::line::insert_line_at(&mut tx, document_id, order, value).await?;
                }
            }
            "replace" => {
                let start = suggestion.start_line_id.as_deref().ok_or_else(|| {
                    AppError::BadRequest("replace suggestion requires start line".into())
                })?;
                let end = suggestion.end_line_id.as_deref().unwrap_or(start);
                let ids = super::line::line_range_tx(&mut tx, document_id, start, end).await?;
                if ids.len() != content.len() {
                    return Err(AppError::Conflict(
                        "replacement line count no longer matches target range".into(),
                    ));
                }
                let timestamp = now();
                for (id, value) in ids.iter().zip(content.iter()) {
                    sqlx::query("UPDATE lines SET content = ?, revision = revision + 1, updated_at = ? WHERE id = ?")
                        .bind(value)
                        .bind(&timestamp)
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            "delete" => {
                let start = suggestion.start_line_id.as_deref().ok_or_else(|| {
                    AppError::BadRequest("delete suggestion requires start line".into())
                })?;
                let end = suggestion.end_line_id.as_deref().unwrap_or(start);
                let ids = super::line::line_range_tx(&mut tx, document_id, start, end).await?;
                let timestamp = now();
                for id in ids {
                    sqlx::query("UPDATE lines SET deleted = 1, revision = revision + 1, updated_at = ? WHERE id = ?")
                        .bind(&timestamp)
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            _ => return Err(AppError::BadRequest("unknown suggestion kind".into())),
        }
        super::audit::touch_document_tx(&mut tx, document_id).await?;
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
        document_id,
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
    super::document::snapshot(pool, document_id).await
}
