use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use crate::error::{AppError, Result};
use crate::models::*;

use super::AUDIT_NOTE_MAX_CHARS;

pub async fn list_history_events(
    pool: &SqlitePool,
    page_id: &str,
    options: HistoryListOptions,
) -> Result<Vec<AuditEvent>> {
    if options.limit < 1 || options.limit > super::HISTORY_MAX_LIMIT {
        return Err(AppError::BadRequest(format!(
            "history limit must be between 1 and {}",
            super::HISTORY_MAX_LIMIT
        )));
    }
    super::page::ensure_page_exists(pool, page_id).await?;

    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT a.id, a.page_id, a.actor_client_id, a.actor_nickname, a.role_mode, a.event_type, a.details_json, a.created_at, n.body AS note_body, n.updated_by_nickname AS note_updated_by_nickname, n.updated_at AS note_updated_at FROM audit_events a LEFT JOIN audit_event_notes n ON n.audit_event_id = a.id WHERE a.page_id = ",
    );
    builder.push_bind(page_id);
    push_history_category_filter(&mut builder, options.category);
    if let Some(from) = options.from {
        builder.push(" AND a.created_at >= ");
        builder.push_bind(from);
    }
    if let Some(to) = options.to {
        builder.push(" AND a.created_at <= ");
        builder.push_bind(to);
    }
    builder.push(" ORDER BY a.created_at DESC LIMIT ");
    builder.push_bind(options.limit);
    Ok(builder
        .build_query_as::<AuditEvent>()
        .fetch_all(pool)
        .await?)
}

pub async fn put_audit_event_note(
    pool: &SqlitePool,
    actor: &Identity,
    page_id: &str,
    audit_event_id: &str,
    body: String,
) -> Result<PageSnapshot> {
    let trimmed = body.trim().to_string();
    if trimmed.chars().count() > AUDIT_NOTE_MAX_CHARS {
        return Err(AppError::BadRequest(format!(
            "audit note must be at most {AUDIT_NOTE_MAX_CHARS} characters"
        )));
    }

    let mut tx = super::audit::begin_write_tx(pool).await?;
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM audit_events WHERE id = ? AND page_id = ?")
            .bind(audit_event_id)
            .bind(page_id)
            .fetch_one(&mut *tx)
            .await?;
    if count.0 == 0 {
        return Err(AppError::NotFound);
    }

    if trimmed.is_empty() {
        sqlx::query("DELETE FROM audit_event_notes WHERE audit_event_id = ? AND page_id = ?")
            .bind(audit_event_id)
            .bind(page_id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("INSERT INTO audit_event_notes (audit_event_id, page_id, body, updated_by_client_id, updated_by_nickname, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(audit_event_id) DO UPDATE SET page_id = excluded.page_id, body = excluded.body, updated_by_client_id = excluded.updated_by_client_id, updated_by_nickname = excluded.updated_by_nickname, updated_at = excluded.updated_at")
            .bind(audit_event_id)
            .bind(page_id)
            .bind(&trimmed)
            .bind(&actor.client_id)
            .bind(&actor.nickname)
            .bind(now())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    super::page::snapshot(pool, page_id).await
}

pub async fn history_diff(
    pool: &SqlitePool,
    page_id: &str,
    from_audit_event_id: &str,
    to_audit_event_id: &str,
) -> Result<HistoryDiff> {
    let from_event = audit_event(pool, page_id, from_audit_event_id).await?;
    let to_event = audit_event(pool, page_id, to_audit_event_id).await?;
    let from_content = version_content(pool, page_id, from_audit_event_id).await?;
    let to_content = version_content(pool, page_id, to_audit_event_id).await?;
    Ok(HistoryDiff {
        from_event,
        to_event,
        from_content,
        to_content,
    })
}

fn push_history_category_filter(
    builder: &mut QueryBuilder<'_, Sqlite>,
    category: HistoryCategory,
) {
    match category {
        HistoryCategory::DocumentComment => {
            builder.push(" AND (");
            push_content_event_filter(builder);
            builder.push(" OR a.event_type LIKE 'comment.%')");
        }
        HistoryCategory::All => {}
        HistoryCategory::Content => {
            builder.push(" AND ");
            push_content_event_filter(builder);
        }
        HistoryCategory::Comment => {
            builder.push(" AND a.event_type LIKE 'comment.%'");
        }
        HistoryCategory::Suggestion => {
            builder.push(" AND a.event_type LIKE 'suggestion.%'");
        }
        HistoryCategory::System => {
            builder.push(" AND NOT (");
            push_content_event_filter(builder);
            builder.push(" OR a.event_type LIKE 'comment.%' OR a.event_type LIKE 'suggestion.%')");
        }
    }
}

fn push_content_event_filter(builder: &mut QueryBuilder<'_, Sqlite>) {
    builder.push(
        "(a.event_type IN ('page.created', 'page.content_updated') OR a.event_type LIKE 'blocks.%' OR a.event_type LIKE 'block.%')",
    );
}

async fn audit_event(
    pool: &SqlitePool,
    page_id: &str,
    audit_event_id: &str,
) -> Result<AuditEvent> {
    Ok(sqlx::query_as::<_, AuditEvent>(
        "SELECT a.id, a.page_id, a.actor_client_id, a.actor_nickname, a.role_mode, a.event_type, a.details_json, a.created_at, n.body AS note_body, n.updated_by_nickname AS note_updated_by_nickname, n.updated_at AS note_updated_at FROM audit_events a LEFT JOIN audit_event_notes n ON n.audit_event_id = a.id WHERE a.page_id = ? AND a.id = ?",
    )
    .bind(page_id)
    .bind(audit_event_id)
    .fetch_one(pool)
    .await?)
}

async fn version_content(
    pool: &SqlitePool,
    page_id: &str,
    audit_event_id: &str,
) -> Result<String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT content FROM page_versions WHERE page_id = ? AND audit_event_id = ?",
    )
    .bind(page_id)
    .bind(audit_event_id)
    .fetch_optional(pool)
    .await?;
    row.map(|(content,)| content)
        .ok_or_else(|| AppError::Conflict("version data not available".into()))
}
