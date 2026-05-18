use chrono::Utc;
use serde_json::Value;
use sqlx::{SqlitePool, Transaction, Sqlite};

use crate::error::Result;
use crate::models::{now, new_id, Identity};
use documosa_core::identity::ActorKind;

pub(crate) async fn begin_write_tx(pool: &SqlitePool) -> Result<Transaction<'_, Sqlite>> {
    Ok(pool.begin_with("BEGIN IMMEDIATE").await?)
}

pub(crate) async fn touch_document_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
) -> Result<()> {
    sqlx::query("UPDATE documents SET updated_at = ? WHERE id = ?")
        .bind(now())
        .bind(document_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn active_content_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
) -> Result<String> {
    Ok(super::line::active_lines_tx(tx, document_id)
        .await?
        .into_iter()
        .map(|line| line.content)
        .collect::<Vec<_>>()
        .join("\n"))
}

pub(crate) async fn audit_tx(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    actor: &Identity,
    event_type: &str,
    details: Value,
) -> Result<()> {
    let enriched_details = match &actor.actor_kind {
        ActorKind::Agent { agent_id, session_ref, task_ref } => {
            let mut d = details;
            if let Some(obj) = d.as_object_mut() {
                obj.insert("actor_kind".into(), serde_json::json!("agent"));
                obj.insert("agent_id".into(), serde_json::json!(agent_id));
                if let Some(sr) = session_ref {
                    obj.insert("session_ref".into(), serde_json::json!(sr));
                }
                if let Some(tr) = task_ref {
                    obj.insert("task_ref".into(), serde_json::json!(tr));
                }
            }
            d
        }
        ActorKind::Human => details,
    };
    let audit_event_id = new_id();
    let timestamp = now();
    sqlx::query("INSERT INTO audit_events (id, document_id, actor_client_id, actor_nickname, role_mode, event_type, details_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&audit_event_id)
        .bind(document_id)
        .bind(&actor.client_id)
        .bind(&actor.nickname)
        .bind(actor.role_mode.as_str())
        .bind(event_type)
        .bind(serde_json::to_string(&enriched_details)?)
        .bind(&timestamp)
        .execute(&mut **tx)
        .await?;
    let content = active_content_tx(tx, document_id).await?;
    sqlx::query(
        "INSERT INTO document_versions (audit_event_id, document_id, content, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&audit_event_id)
    .bind(document_id)
    .bind(content)
    .bind(timestamp)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn prune_expired_locks_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    sqlx::query("DELETE FROM locks WHERE expires_at <= ?")
        .bind(Utc::now().to_rfc3339())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
