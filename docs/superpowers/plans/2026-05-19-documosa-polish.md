# Documosa v1.0 Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish documosa reference implementation: split db.rs, implement DocumosaOps trait, enforce permissions via core, record ActorKind in audit, and add fine-grained WebSocket events (breaking).

**Architecture:** Five independent changes applied sequentially. db.rs splits into 10 focused modules under `src/db/` without changing public API. Permissions switch from hardcoded `require_writer()` to `documosa_core::permissions::is_allowed()`. `SqlitePool` implements `DocumosaOps` via a new `ops.rs`. `audit_tx` enriches details_json with ActorKind info. `ServerEvent` gets specific variants replacing generic `DocumentChanged`.

**Tech Stack:** Rust 2024 edition, axum 0.8, sqlx 0.8, tokio, TypeScript React frontend.

---

### Task 1: Create db/ directory and text utilities

**Files:**
- Create: `src/db/text.rs`
- Modify: `src/db.rs` (extract text functions)

- [ ] **Step 1: Create `src/db/text.rs` with text utility functions**

```rust
// src/db/text.rs — pure text utilities, no SQL dependencies

pub fn split_lines_preserve_trailing(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return vec![];
    }
    let mut parts: Vec<&str> = content.split('\n').collect();
    for line in &mut parts {
        if let Some(stripped) = line.strip_suffix('\r') {
            *line = stripped;
        }
    }
    if content.chars().all(|c| c == '\n' || c == '\r') {
        parts.pop();
    }
    parts
}

pub fn text_summary(value: &str) -> String {
    value.chars().take(120).collect()
}

pub fn text_len(value: &str) -> usize {
    value.chars().count()
}

pub fn line_count(value: &str) -> usize {
    split_lines_preserve_trailing(value).len()
}
```

- [ ] **Step 2: Delete text functions from `src/db.rs`** — remove the 4 functions (`split_lines_preserve_trailing`, `text_summary`, `text_len`, `line_count`) and the unused `parse_time` function from the end of db.rs

- [ ] **Step 3: Add `mod text;` and pub use to `src/db.rs`**

Add at top of db.rs:
```rust
mod text;
pub(crate) use text::*;
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/db/text.rs src/db.rs
git commit -m "refactor: extract text utilities into db/text.rs"
```

---

### Task 2: Extract document operations to db/document.rs

**Files:**
- Create: `src/db/document.rs`
- Modify: `src/db.rs`

- [ ] **Step 1: Create `src/db/document.rs`**

Move these functions from db.rs: `create_document`, `list_documents`, `update_document_title`, `snapshot`, `export_document`, `ensure_document_exists`.

File header:
```rust
use chrono::Utc;
use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite};

use crate::error::{AppError, Result};
use crate::models::*;

use super::text::*;
use super::audit::audit_tx;
use super::line::{insert_line_at, split_and_insert_lines};

// ... all document functions moved here verbatim ...
```

Note: `create_document` calls `insert_line_at` and `split_lines_preserve_trailing` — both now in sub-modules. Add `use super::line::insert_line_at;` and use from `super::text`.

- [ ] **Step 2: Delete document functions from `src/db.rs`**

- [ ] **Step 3: Add to `src/db.rs`**

```rust
mod document;
pub use document::*;
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/db/document.rs src/db.rs
git commit -m "refactor: extract document operations into db/document.rs"
```

---

### Task 3: Extract audit operations to db/audit.rs

**Files:**
- Create: `src/db/audit.rs`
- Modify: `src/db.rs`

- [ ] **Step 1: Create `src/db/audit.rs`**

Move: `audit_tx`, `begin_write_tx`, `touch_document_tx`, `active_content_tx`, `prune_expired_locks_tx`.

```rust
use chrono::Utc;
use serde_json::Value;
use sqlx::{SqlitePool, Transaction, Sqlite, SqlitePool};

use crate::error::{AppError, Result};
use crate::models::{now, new_id, Identity};

use super::text::*;
use super::line::active_content_tx_helper;

// ... all audit functions moved verbatim ...
```

Note: `audit_tx` calls `active_content_tx` which calls `active_lines_tx`. Both need to be accessible. Since `active_content_tx` is only used by audit, move it here too. `active_lines_tx` stays in line module — audit will call it via `super::line::active_lines_tx`.

Actually, keep `active_content_tx` in audit.rs and have it call `super::line::active_lines_tx`.

- [ ] **Step 2: Delete audit functions from `src/db.rs`**

- [ ] **Step 3: Add to `src/db.rs`**

```rust
mod audit;
pub(crate) use audit::{audit_tx, begin_write_tx, touch_document_tx, prune_expired_locks_tx};
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/db/audit.rs src/db.rs
git commit -m "refactor: extract audit operations into db/audit.rs"
```

---

### Task 4: Extract line operations to db/line.rs

**Files:**
- Create: `src/db/line.rs`
- Modify: `src/db.rs`

- [ ] **Step 1: Create `src/db/line.rs`**

Move: `insert_lines`, `replace_lines`, `delete_lines`, `update_content`, `insert_line_at`, `insertion_orders_tx`, `insertion_bounds_tx`, `active_lines_tx`, `ensure_line_exists_tx`, `get_line_tx`, `order_for_line_tx`, `renumber_active_tx`, `line_range_tx`, `require_writer`.

```rust
use chrono::Utc;
use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite, QueryBuilder};

use crate::error::{AppError, Result};
use crate::models::*;

use super::text::*;
use super::audit::{audit_tx, touch_document_tx, begin_write_tx};
use super::lock::ensure_unlocked_tx;

const LINE_ORDER_STEP: i64 = 1000;

// ... all line functions moved verbatim ...
```

- [ ] **Step 2: Add `pub(crate) use` for internal helpers in db.rs**

```rust
mod line;
pub use line::*;
// Also expose internal helpers for tests
pub(crate) use line::{insert_line_at, active_lines_tx, ensure_line_exists_tx, get_line_tx, insertion_orders_tx};
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add src/db/line.rs src/db.rs
git commit -m "refactor: extract line operations into db/line.rs"
```

---

### Task 5: Extract lock, comment, suggestion, history modules

**Files:**
- Create: `src/db/lock.rs`, `src/db/comment.rs`, `src/db/suggestion.rs`, `src/db/history.rs`
- Modify: `src/db.rs`

- [ ] **Step 1: Create `src/db/lock.rs`**

Move: `heartbeat_locks`, `release_locks`, `locks`, `ensure_unlocked_tx`, `prune_expired_locks_tx` (if not already in audit).

```rust
use chrono::{Utc, Duration};
use serde_json::json;
use sqlx::{SqlitePool, Transaction, Sqlite, QueryBuilder};

use crate::error::{AppError, Result};
use crate::models::*;

use super::audit::{audit_tx, prune_expired_locks_tx};
use super::line::ensure_line_exists_tx;

// ... lock functions ...
```

- [ ] **Step 2: Create `src/db/comment.rs`**

Move: `CommentDraft`, `create_comment`, `update_comment`, `delete_comment`, `reply_comment`, `resolve_comment`, `get_comment_tx`.

- [ ] **Step 3: Create `src/db/suggestion.rs`**

Move: `create_suggestion`, `decide_suggestion`.

- [ ] **Step 4: Create `src/db/history.rs`**

Move: `list_history_events`, `put_audit_event_note`, `history_diff`, `push_history_category_filter`, `push_content_event_filter`, `audit_event`, `version_content`.

- [ ] **Step 5: Add mod declarations to `src/db.rs`**

```rust
mod text;
pub(crate) use text::*;
mod audit;
pub(crate) use audit::{audit_tx, begin_write_tx, touch_document_tx, prune_expired_locks_tx};
mod document;
pub use document::*;
mod line;
pub use line::*;
mod lock;
pub use lock::*;
mod comment;
pub use comment::*;
mod suggestion;
pub use suggestion::*;
mod history;
pub use history::*;
```

- [ ] **Step 6: Delete all moved functions from `src/db.rs`** — keep only `connect`, `connect_memory`, `migrate`, constants, and mod declarations

- [ ] **Step 7: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add src/db/ src/db.rs
git commit -m "refactor: split db.rs into focused modules (lock, comment, suggestion, history)"
```

---

### Task 6: Complete db/mod.rs — convert db.rs to db/mod.rs

**Files:**
- Rename: `src/db.rs` → `src/db/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Move `src/db.rs` to `src/db/mod.rs`**

```bash
mv src/db.rs src/db/mod.rs
```

- [ ] **Step 2: `src/lib.rs` already has `pub mod db;`** — no change needed (it auto-discovers `db/mod.rs`)

- [ ] **Step 3: Verify compilation and tests**

```bash
cargo check --all-targets 2>&1
cargo test 2>&1
```

Expected: all 54 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/db/mod.rs src/db.rs
git commit -m "refactor: convert db.rs to db/mod.rs module directory"
```

---

### Task 7: Implement require_permission and replace require_writer

**Files:**
- Create: `src/db/permission.rs`
- Modify: `src/db/line.rs`, `src/db/lock.rs`, `src/db/comment.rs`, `src/db/suggestion.rs`

- [ ] **Step 1: Create `src/db/permission.rs`**

```rust
use documosa_core::identity::RoleMode;
use documosa_core::permissions::is_allowed;
use crate::error::{AppError, Result};

pub fn require_permission(actor: &documosa_core::identity::Identity, operation: &str) -> Result<()> {
    if !is_allowed(actor.role_mode, operation) {
        return Err(AppError::Forbidden(format!(
            "role {} is not allowed to perform {operation}",
            actor.role_mode.as_str()
        )));
    }
    Ok(())
}
```

- [ ] **Step 2: Add to `src/db/mod.rs`**

```rust
mod permission;
pub(crate) use permission::require_permission;
```

- [ ] **Step 3: Replace `require_writer(actor)?` calls**

In `src/db/line.rs`:
- `update_content`: replace `require_writer(actor)?` with `require_permission(actor, "replace_lines")?`
- `insert_lines`: replace with `require_permission(actor, "insert_lines")?`
- `replace_lines`: replace with `require_permission(actor, "replace_lines")?`
- `delete_lines`: replace with `require_permission(actor, "delete_lines")?`

In `src/db/lock.rs`:
- `heartbeat_locks`: replace with `require_permission(actor, "lock_lines")?`
- `release_locks`: replace with `require_permission(actor, "release_locks")?`

In `src/db/comment.rs`:
- `resolve_comment`: replace with `require_permission(actor, "resolve_comment")?`

In `src/db/suggestion.rs`:
- `decide_suggestion`: replace with `require_permission(actor, if accept { "accept_suggestion" } else { "reject_suggestion" })?`

- [ ] **Step 4: Remove `require_writer` function from `src/db/line.rs`**

- [ ] **Step 5: Verify compilation and tests**

```bash
cargo test 2>&1
```

Expected: all 54 tests pass (no behavior change for Writer callers). The `document_lines_locks_comments_suggestions_and_audit_work` test already has a Reviewer attempting to edit lines — that should still fail with Forbidden.

- [ ] **Step 6: Commit**

```bash
git add src/db/
git commit -m "refactor: replace require_writer with core permission matrix"
```

---

### Task 8: Record ActorKind in audit events

**Files:**
- Modify: `src/db/audit.rs`

- [ ] **Step 1: Update `audit_tx` to enrich details with ActorKind**

In `src/db/audit.rs`, inside `audit_tx`, add before the INSERT:

```rust
use documosa_core::identity::ActorKind;

// Inside audit_tx, before serializing details to string:
let mut enriched = details;
if let Some(obj) = enriched.as_object_mut() {
    match &actor.actor_kind {
        ActorKind::Agent { agent_id, session_ref, task_ref } => {
            obj.insert("actor_kind".into(), serde_json::json!("agent"));
            obj.insert("agent_id".into(), serde_json::json!(agent_id));
            if let Some(sr) = session_ref {
                obj.insert("session_ref".into(), serde_json::json!(sr));
            }
            if let Some(tr) = task_ref {
                obj.insert("task_ref".into(), serde_json::json!(tr));
            }
        }
        ActorKind::Human => {}
    }
}
// Then use `enriched` instead of `details` in serde_json::to_string
```

- [ ] **Step 2: Verify compilation and tests**

```bash
cargo test 2>&1
```

Expected: all 54 tests pass (existing tests use Human identity, details unchanged).

- [ ] **Step 3: Commit**

```bash
git add src/db/audit.rs
git commit -m "feat: record ActorKind (agent_id, session_ref, task_ref) in audit details"
```

---

### Task 9: Implement DocumosaOps for SqlitePool

**Files:**
- Create: `src/db/ops.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Create `src/db/ops.rs`**

```rust
use sqlx::SqlitePool;
use documosa_core::error::ProtocolError;
use documosa_core::identity::Identity;
use documosa_core::models::*;
use documosa_core::operations::{DocumosaOps, SuggestionKind};

use crate::error::AppError;
use super::*;

impl DocumosaOps for SqlitePool {
    async fn create_document(
        &self, actor: &Identity, title: String, content: String,
    ) -> Result<DocumentSnapshot, ProtocolError> {
        document::create_document(self, actor, title, content).await.map_err(|e| ProtocolError::from(e))
    }

    async fn list_documents(&self) -> Result<Vec<Document>, ProtocolError> {
        document::list_documents(self).await.map_err(|e| ProtocolError::from(e))
    }

    async fn get_document(&self, document_id: &str) -> Result<DocumentSnapshot, ProtocolError> {
        document::snapshot(self, document_id).await.map_err(|e| ProtocolError::from(e))
    }

    async fn export_document(&self, document_id: &str) -> Result<String, ProtocolError> {
        document::export_document(self, document_id).await.map_err(|e| ProtocolError::from(e))
    }

    async fn update_title(&self, actor: &Identity, document_id: &str, title: &str) -> Result<(), ProtocolError> {
        document::update_document_title(self, actor, document_id, title).await.map_err(|e| ProtocolError::from(e))
    }

    async fn insert_lines(
        &self, actor: &Identity, document_id: &str,
        anchor_line_id: Option<&str>, lines: Vec<String>,
    ) -> Result<Vec<Line>, ProtocolError> {
        let snap = line::insert_lines(self, actor, document_id, anchor_line_id.map(str::to_string), lines)
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.lines)
    }

    async fn replace_lines(
        &self, actor: &Identity, document_id: &str,
        line_ids: Vec<&str>, new_contents: Vec<String>, _base_revisions: Vec<BaseRevision>,
    ) -> Result<Vec<Line>, ProtocolError> {
        let ids = line_ids.into_iter().map(str::to_string).collect();
        let snap = line::replace_lines(self, actor, document_id, ids, new_contents)
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.lines)
    }

    async fn delete_lines(
        &self, actor: &Identity, document_id: &str, line_ids: Vec<&str>,
    ) -> Result<Vec<Line>, ProtocolError> {
        let ids = line_ids.into_iter().map(str::to_string).collect();
        let snap = line::delete_lines(self, actor, document_id, ids)
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.lines)
    }

    async fn lock_lines(
        &self, actor: &Identity, document_id: &str,
        line_ids: Vec<&str>, _ttl_seconds: i64,
    ) -> Result<Vec<LineLock>, ProtocolError> {
        let ids = line_ids.into_iter().map(str::to_string).collect();
        lock::heartbeat_locks(self, actor, document_id, ids)
            .await.map_err(|e| ProtocolError::from(e))
    }

    async fn heartbeat_locks(
        &self, actor: &Identity, document_id: &str, line_ids: Vec<&str>,
    ) -> Result<Vec<LineLock>, ProtocolError> {
        let ids = line_ids.into_iter().map(str::to_string).collect();
        lock::heartbeat_locks(self, actor, document_id, ids)
            .await.map_err(|e| ProtocolError::from(e))
    }

    async fn release_locks(
        &self, actor: &Identity, document_id: &str, line_ids: Vec<&str>,
    ) -> Result<(), ProtocolError> {
        let ids = line_ids.into_iter().map(str::to_string).collect();
        lock::release_locks(self, actor, document_id, ids)
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(())
    }

    async fn create_comment(
        &self, actor: &Identity, document_id: &str,
        start_line_id: &str, end_line_id: &str,
        start_column: Option<i64>, end_column: Option<i64>, body: &str,
    ) -> Result<Comment, ProtocolError> {
        let snap = comment::create_comment(self, actor, document_id, comment::CommentDraft {
            start_line_id: start_line_id.to_string(),
            end_line_id: end_line_id.to_string(),
            start_column,
            end_column,
            body: body.to_string(),
        }).await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.comments.into_iter().next().unwrap())
    }

    async fn reply_comment(
        &self, actor: &Identity, document_id: &str, comment_id: &str, body: &str,
    ) -> Result<CommentReply, ProtocolError> {
        let snap = comment::reply_comment(self, actor, document_id, comment_id, body.to_string())
            .await.map_err(|e| ProtocolError::from(e))?;
        // Return the last reply (most recently created)
        Ok(snap.replies.into_iter().last().unwrap())
    }

    async fn update_comment(
        &self, actor: &Identity, document_id: &str, comment_id: &str, body: &str,
    ) -> Result<Comment, ProtocolError> {
        let snap = comment::update_comment(self, actor, document_id, comment_id, body.to_string())
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.comments.into_iter().find(|c| c.id == comment_id).unwrap())
    }

    async fn resolve_comment(
        &self, actor: &Identity, document_id: &str, comment_id: &str,
    ) -> Result<Comment, ProtocolError> {
        let snap = comment::resolve_comment(self, actor, document_id, comment_id)
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.comments.into_iter().find(|c| c.id == comment_id).unwrap())
    }

    async fn create_suggestion(
        &self, actor: &Identity, document_id: &str,
        kind: SuggestionKind, anchor_line_id: Option<&str>,
        start_line_id: Option<&str>, end_line_id: Option<&str>,
        new_content: &str, _base_revisions: Vec<BaseRevision>,
    ) -> Result<Suggestion, ProtocolError> {
        let kind_str = match kind {
            SuggestionKind::InsertLines => "insert",
            SuggestionKind::ReplaceLines => "replace",
            SuggestionKind::DeleteLines => "delete",
        };
        let content: Vec<String> = new_content.lines().map(str::to_string).collect();
        let snap = suggestion::create_suggestion(
            self, actor, document_id,
            kind_str.to_string(),
            anchor_line_id.map(str::to_string),
            start_line_id.map(str::to_string),
            end_line_id.map(str::to_string),
            content,
        ).await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.suggestions.into_iter().last().unwrap())
    }

    async fn accept_suggestion(
        &self, actor: &Identity, document_id: &str, suggestion_id: &str,
    ) -> Result<Suggestion, ProtocolError> {
        let snap = suggestion::decide_suggestion(self, actor, document_id, suggestion_id, true)
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.suggestions.into_iter().find(|s| s.id == suggestion_id).unwrap())
    }

    async fn reject_suggestion(
        &self, actor: &Identity, document_id: &str, suggestion_id: &str,
    ) -> Result<Suggestion, ProtocolError> {
        let snap = suggestion::decide_suggestion(self, actor, document_id, suggestion_id, false)
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(snap.suggestions.into_iter().find(|s| s.id == suggestion_id).unwrap())
    }

    async fn list_history(
        &self, document_id: &str, category: HistoryCategory,
        from: Option<&str>, to: Option<&str>, limit: i64,
    ) -> Result<Vec<AuditEvent>, ProtocolError> {
        history::list_history_events(self, document_id, HistoryListOptions {
            category,
            from: from.map(str::to_string),
            to: to.map(str::to_string),
            limit,
        }).await.map_err(|e| ProtocolError::from(e))
    }

    async fn history_diff(
        &self, document_id: &str, from_event_id: &str, to_event_id: &str,
    ) -> Result<HistoryDiff, ProtocolError> {
        history::history_diff(self, document_id, from_event_id, to_event_id)
            .await.map_err(|e| ProtocolError::from(e))
    }

    async fn set_audit_note(
        &self, actor: &Identity, document_id: &str, audit_event_id: &str, body: &str,
    ) -> Result<(), ProtocolError> {
        history::put_audit_event_note(self, actor, document_id, audit_event_id, body.to_string())
            .await.map_err(|e| ProtocolError::from(e))?;
        Ok(())
    }
}

impl From<AppError> for ProtocolError {
    fn from(err: AppError) -> Self {
        match err {
            AppError::Protocol(e) => e,
            AppError::BadRequest(msg) => ProtocolError::BadRequest(msg),
            AppError::Forbidden(msg) => ProtocolError::Forbidden(msg),
            AppError::Conflict(msg) => ProtocolError::Conflict(msg),
            AppError::NotFound => ProtocolError::NotFound,
            AppError::Unauthorized => ProtocolError::Forbidden("unauthorized".into()),
            AppError::Sqlx(_) | AppError::Io(_) | AppError::Json(_) | AppError::Anyhow(_) => {
                ProtocolError::BadRequest("internal error".into())
            }
        }
    }
}
```

- [ ] **Step 2: Add to `src/db/mod.rs`**

```rust
mod ops;
```

- [ ] **Step 3: Add `documosa-core` dependency to `src/lib.rs` exports**

Verify `pub use documosa_core::operations::DocumosaOps;` is accessible. The trait is re-exported via `documosa_core`. No additional re-export needed — external crates use `use documosa_core::operations::DocumosaOps;` directly.

- [ ] **Step 4: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/db/ops.rs src/db/mod.rs
git commit -m "feat: implement DocumosaOps trait for SqlitePool"
```

---

### Task 10: Redesign ServerEvent with specific variants

**Files:**
- Modify: `src/realtime.rs`

- [ ] **Step 1: Replace `ServerEvent` enum**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Presence {
        document_id: String,
        users: Vec<Presence>,
    },
    LinesInserted {
        document_id: String,
        line_ids: Vec<String>,
        after_line_id: Option<String>,
    },
    LinesReplaced {
        document_id: String,
        line_ids: Vec<String>,
    },
    LinesDeleted {
        document_id: String,
        line_ids: Vec<String>,
    },
    CommentCreated {
        document_id: String,
        comment_id: String,
    },
    CommentResolved {
        document_id: String,
        comment_id: String,
    },
    SuggestionCreated {
        document_id: String,
        suggestion_id: String,
    },
    SuggestionDecided {
        document_id: String,
        suggestion_id: String,
        accepted: bool,
    },
    DocumentTitleUpdated {
        document_id: String,
        title: String,
    },
    LocksChanged {
        document_id: String,
    },
}
```

- [ ] **Step 2: Replace `EventHub::document_changed` with specific methods**

```rust
impl EventHub {
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> { self.tx.subscribe() }

    pub fn lines_inserted(&self, document_id: &str, line_ids: &[String], after_line_id: Option<&str>) {
        let _ = self.tx.send(ServerEvent::LinesInserted {
            document_id: document_id.to_string(),
            line_ids: line_ids.to_vec(),
            after_line_id: after_line_id.map(str::to_string),
        });
    }

    pub fn lines_replaced(&self, document_id: &str, line_ids: &[String]) {
        let _ = self.tx.send(ServerEvent::LinesReplaced {
            document_id: document_id.to_string(),
            line_ids: line_ids.to_vec(),
        });
    }

    pub fn lines_deleted(&self, document_id: &str, line_ids: &[String]) {
        let _ = self.tx.send(ServerEvent::LinesDeleted {
            document_id: document_id.to_string(),
            line_ids: line_ids.to_vec(),
        });
    }

    pub fn comment_created(&self, document_id: &str, comment_id: &str) {
        let _ = self.tx.send(ServerEvent::CommentCreated {
            document_id: document_id.to_string(),
            comment_id: comment_id.to_string(),
        });
    }

    pub fn comment_resolved(&self, document_id: &str, comment_id: &str) {
        let _ = self.tx.send(ServerEvent::CommentResolved {
            document_id: document_id.to_string(),
            comment_id: comment_id.to_string(),
        });
    }

    pub fn suggestion_created(&self, document_id: &str, suggestion_id: &str) {
        let _ = self.tx.send(ServerEvent::SuggestionCreated {
            document_id: document_id.to_string(),
            suggestion_id: suggestion_id.to_string(),
        });
    }

    pub fn suggestion_decided(&self, document_id: &str, suggestion_id: &str, accepted: bool) {
        let _ = self.tx.send(ServerEvent::SuggestionDecided {
            document_id: document_id.to_string(),
            suggestion_id: suggestion_id.to_string(),
            accepted,
        });
    }

    pub fn title_updated(&self, document_id: &str, title: &str) {
        let _ = self.tx.send(ServerEvent::DocumentTitleUpdated {
            document_id: document_id.to_string(),
            title: title.to_string(),
        });
    }

    pub fn locks_changed(&self, document_id: &str) {
        let _ = self.tx.send(ServerEvent::LocksChanged {
            document_id: document_id.to_string(),
        });
    }

    // join/leave unchanged
}
```

- [ ] **Step 3: Update websocket handler event matching**

In `pub async fn websocket`, update the event matching:

```rust
let event_doc = match &event {
    ServerEvent::Presence { document_id, .. } => document_id,
    ServerEvent::LinesInserted { document_id, .. } => document_id,
    ServerEvent::LinesReplaced { document_id, .. } => document_id,
    ServerEvent::LinesDeleted { document_id, .. } => document_id,
    ServerEvent::CommentCreated { document_id, .. } => document_id,
    ServerEvent::CommentResolved { document_id, .. } => document_id,
    ServerEvent::SuggestionCreated { document_id, .. } => document_id,
    ServerEvent::SuggestionDecided { document_id, .. } => document_id,
    ServerEvent::DocumentTitleUpdated { document_id, .. } => document_id,
    ServerEvent::LocksChanged { document_id } => document_id,
};
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/realtime.rs
git commit -m "refactor: replace generic DocumentChanged with specific WebSocket events (breaking)"
```

---

### Task 11: Update api.rs event hub calls

**Files:**
- Modify: `src/api.rs`

- [ ] **Step 1: Replace all `hub.document_changed(...)` calls**

| Old call | New call |
|----------|----------|
| `hub.document_changed(&snapshot.document.id, "document.created")` | `hub.lines_inserted(&snapshot.document.id, &[], None)` (or no event — document creation triggers a full client-side init) |
| `hub.document_changed(&document_id, "document.content_updated")` | Hub event is less useful here since it's a full replace — keep as generic. Add a new `document_content_updated` variant or use `hub.lines_changed(&document_id, &all_ids)` |
| `hub.document_changed(&document_id, "lines.inserted")` | `hub.lines_inserted(&document_id, &inserted_line_ids, after_line_id.as_deref())` |
| `hub.document_changed(&document_id, "lines.replaced")` | `hub.lines_replaced(&document_id, &line_ids)` |
| `hub.document_changed(&document_id, "lines.deleted")` | `hub.lines_deleted(&document_id, &line_ids)` |
| `hub.document_changed(&document_id, "comment.created")` | `hub.comment_created(&document_id, &comment_id)` |
| `hub.document_changed(&document_id, "comment.updated")` | `hub.comment_resolved(...)` or keep as a generic — we don't have a specific update event. Add a `ContentChanged { document_id }` variant for non-critical updates. |
| `hub.document_changed(&document_id, "comment.deleted")` | Delete is rare — no specific event needed, use generic `ContentChanged` |
| `hub.document_changed(&document_id, "comment.replied")` | No specific event needed |
| `hub.document_changed(&document_id, "comment.resolved")` | `hub.comment_resolved(&document_id, &comment_id)` |
| `hub.document_changed(&document_id, "suggestion.created")` | `hub.suggestion_created(&document_id, &suggestion_id)` |
| `hub.document_changed(&document_id, "suggestion.accepted")` | `hub.suggestion_decided(&document_id, &suggestion_id, true)` |
| `hub.document_changed(&document_id, "suggestion.rejected")` | `hub.suggestion_decided(&document_id, &suggestion_id, false)` |
| `hub.document_changed(&document_id, "locks.heartbeat")` | `hub.locks_changed(&document_id)` |
| `hub.document_changed(&document_id, "locks.released")` | `hub.locks_changed(&document_id)` |
| `hub.document_changed(&document_id, "audit.note.updated")` | Generic `ContentChanged` variant |

Add a catch-all variant for events without specific fields:
```rust
ContentChanged { document_id: String }
```

- [ ] **Step 2: Extract line IDs and comment IDs from snapshots before calling hub**

For `insert_lines`: capture inserted line IDs from returned snapshot
For `create_comment`: capture comment ID from returned snapshot

- [ ] **Step 3: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add src/api.rs src/realtime.rs
git commit -m "refactor: update api.rs to use specific WebSocket event methods"
```

---

### Task 12: Update mcp.rs event hub calls

**Files:**
- Modify: `src/mcp.rs`

- [ ] **Step 1: Replace all `hub.document_changed(...)` calls**

Same mapping pattern as Task 11:

```rust
// "document.created" → no event needed (MCP client gets result directly)
// "lines.inserted" → extract line_ids from snap, call hub.lines_inserted
// "lines.replaced" → hub.lines_replaced
// "lines.deleted" → hub.lines_deleted
// "comment.created" → hub.comment_created
// "comment.replied" → hub.content_changed (generic)
// "comment.resolved" → hub.comment_resolved
// "suggestion.created" → hub.suggestion_created
// "suggestion.accepted/rejected" → hub.suggestion_decided
// "audit.note.updated" → hub.content_changed
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add src/mcp.rs
git commit -m "refactor: update mcp.rs to use specific WebSocket event methods"
```

---

### Task 13: Update frontend WebSocket handler

**Files:**
- Modify: `web/src/App.tsx` (WebSocket onmessage handler, lines 814-842)

- [ ] **Step 1: Replace the socket.onmessage handler**

Current handler (lines 814-842):
```typescript
socket.onmessage = (message) => {
  let topic = ''
  try {
    const event = JSON.parse(message.data) as { type?: string; topic?: string; users?: PresenceUser[] }
    if (event.type === 'document_changed') topic = event.topic ?? ''
    if (event.type === 'presence' && event.users) {
      setOnlineUsers(event.users)
      return
    }
  } catch {
    topic = ''
  }
  void request<Snapshot>(`/api/documents/${snapshot.document.id}`)
    .then((next) => {
      void refreshDocuments().catch(() => undefined)
      if (topic === 'audit.note.updated') {
        applySnapshot(next, false)
        return
      }
      if (dirtyRef.current) {
        setRemoteConflict(true)
        setStatus(t('conflict.message'))
        return
      }
      applySnapshot(next, true)
    })
    .catch((error) => setStatus(error.message))
}
```

New handler:
```typescript
type WsEvent = {
  type: string
  document_id: string
  users?: PresenceUser[]
  line_ids?: string[]
  after_line_id?: string | null
  comment_id?: string
  suggestion_id?: string
  accepted?: boolean
  title?: string
}

socket.onmessage = (message) => {
  let event: WsEvent
  try { event = JSON.parse(message.data) as WsEvent } catch { return }

  switch (event.type) {
    case 'presence':
      if (event.users) setOnlineUsers(event.users)
      return

    case 'comment_created':
    case 'comment_resolved':
      // Incremental: refresh snapshot but preserve editor state
      void request<Snapshot>(`/api/documents/${snapshot.document.id}`)
        .then((next) => applySnapshot(next, false))
        .catch((error) => setStatus(error.message))
      void refreshDocuments().catch(() => undefined)
      return

    case 'lines_inserted':
    case 'lines_replaced':
    case 'lines_deleted':
      if (dirtyRef.current) {
        setRemoteConflict(true)
        setStatus(t('conflict.message'))
        return
      }
      void request<Snapshot>(`/api/documents/${snapshot.document.id}`)
        .then((next) => {
          void refreshDocuments().catch(() => undefined)
          applySnapshot(next, true)
        })
        .catch((error) => setStatus(error.message))
      return

    case 'suggestion_created':
    case 'suggestion_decided':
    case 'document_title_updated':
    case 'locks_changed':
    case 'content_changed':
      void request<Snapshot>(`/api/documents/${snapshot.document.id}`)
        .then((next) => applySnapshot(next, false))
        .catch((error) => setStatus(error.message))
      void refreshDocuments().catch(() => undefined)
      return
  }
}
```

- [ ] **Step 2: Verify frontend builds**

```bash
cd web && npm run build 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add web/src/App.tsx
git commit -m "refactor: update frontend to handle specific WebSocket event types"
```

---

### Task 14: Final verification

- [ ] **Step 1: Check all targets compile**

```bash
cargo check --all-targets 2>&1
```

Expected: no errors.

- [ ] **Step 2: Run all tests**

```bash
cargo test 2>&1
```

Expected: all 54 tests pass.

- [ ] **Step 3: Build binary**

```bash
cargo build 2>&1
```

Expected: binary builds successfully.

- [ ] **Step 4: Commit any remaining changes**

```bash
git add -A
git commit -m "chore: final verification after documosa polish"
```
