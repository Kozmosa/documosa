# Documosa v2.0 Bugfix & Notion API Hardening — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all 13 issues from the E2E audit: auth bypass, data integrity bugs (lock bypass, cascade delete, float precision, multi-increment revision, concurrent gap), error leakage, input validation, and Notion API compatibility gaps.

**Architecture:** 4 phases applied bottom-up. Phase 1 (auth) touches api/ layer only. Phase 2 (data integrity) touches db/block.rs and db/lock.rs. Phase 3 (robustness) touches both api/ and db/. Phase 4 (Notion compat) is pure api/ layer refactoring.

**Tech Stack:** Rust 2024, axum 0.8, sqlx 0.8, serde_json.

---

### Task 1: Auth — enforce JWT on all GET endpoints

**Files:**
- Modify: `src/api/pages.rs:42-73`
- Modify: `src/api/blocks.rs` (get_block, list_children handlers)
- Modify: `src/api/comments.rs` (list_comments handler)
- Modify: `src/api/history.rs` (list_history, history_diff handlers)
- Modify: `src/api/export.rs` (export_markdown handler)

- [ ] **Step 1: Add `MmdashIdentity` extractor to all unprotected GET handlers**

In `src/api/pages.rs`, change:
```rust
async fn list_pages(
    State(state): State<AppState>,
) -> Result<impl IntoResponse> {
```
To:
```rust
async fn list_pages(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
) -> Result<impl IntoResponse> {
```

Same pattern for `get_page`, `get_snapshot` in pages.rs, `get_block` and `list_children` in blocks.rs, `list_comments` in comments.rs, `list_history` and `history_diff` in history.rs, `export_markdown` in export.rs.

Note: some of these already have `MmdashIdentity` (like `create_page`, `update_page`). Only add it where it's currently missing.

- [ ] **Step 2: Verify auth enforcement**

```bash
cd /home/xuyang/code/kozbox/apps/documosa
JWT_SECRET=test cargo run -- serve --port 14318 --data-dir /tmp/doc-e2e &
sleep 3
# Should return 401
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:14318/pages
# Should return 401
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:14318/pages/any-id
kill %1 2>/dev/null
```
Expected: both return 401.

- [ ] **Step 3: Run tests**

```bash
cargo test 2>&1
```
Tests may fail because they now need JWT on GET endpoints. Update tests in `tests/integration.rs` to include `Authorization: Bearer <token>` header on all GET requests.

- [ ] **Step 4: Commit**

```bash
git add src/api/ tests/integration.rs
git commit -m "fix: enforce JWT auth on all GET endpoints"
```

---

### Task 2: Lock enforcement — check locks before update_block/delete_block

**Files:**
- Modify: `src/db/block.rs:181-188` (update_block)
- Modify: `src/db/block.rs:257-264` (delete_block)

- [ ] **Step 1: Add lock check to `update_block`**

After `get_block_tx` and before the UPDATE statements, insert:
```rust
super::lock::ensure_unlocked_tx(&mut tx, actor, &page_id, std::slice::from_ref(block_id)).await?;
```

- [ ] **Step 2: Add lock check to `delete_block`**

Same insertion after `get_block_tx`:
```rust
super::lock::ensure_unlocked_tx(&mut tx, actor, &page_id, std::slice::from_ref(block_id)).await?;
```

- [ ] **Step 3: Add test for lock bypass**

In `tests/integration.rs`, add a test that locks a block with Writer A, then attempts update by Writer B — must return 409 Conflict.

- [ ] **Step 4: Run tests and commit**

```bash
cargo test 2>&1
git add src/db/block.rs tests/integration.rs
git commit -m "fix: enforce block lock checks on update_block and delete_block"
```

---

### Task 3: Cascade delete — soft-delete all descendants

**Files:**
- Modify: `src/db/block.rs:257-295` (delete_block)

- [ ] **Step 1: Add recursive descendant soft-delete to `delete_block`**

After the initial soft-delete of the target block and BEFORE `touch_page_tx`, add:
```rust
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
```

- [ ] **Step 2: Add test for cascade delete**

Create page with nested blocks (parent → child → grandchild). Delete parent. Verify all three are marked deleted=1.

- [ ] **Step 3: Run tests and commit**

```bash
cargo test 2>&1
git add src/db/block.rs tests/integration.rs
git commit -m "fix: cascade soft-delete all descendants when deleting a block"
```

---

### Task 4: Float precision — renumber page when gap exhausted

**Files:**
- Modify: `src/db/block.rs` — add `renumber_page_blocks_tx` helper
- Modify: `src/db/block.rs:90-178` (append_blocks)

- [ ] **Step 1: Add `renumber_page_blocks_tx` helper**

```rust
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
```

- [ ] **Step 2: Trigger renumber when gap is too small**

In `append_blocks`, after computing `gap = next - base_order`, add:
```rust
if gap < 1e-12 || gap <= blocks.len() as f64 {
    renumber_page_blocks_tx(&mut tx, page_id).await?;
    // Re-read base_order and next_order after renumber
    base_order = if let Some(after_id) = after {
        get_order_tx(&mut tx, after_id).await?
    } else { 0.0 };
    next_order = if let Some(after_id) = after {
        let after_order = get_order_tx(&mut tx, after_id).await?;
        next_order_tx(&mut tx, page_id, after_order).await?
    } else { None };
}
```

Then recompute `gap` and `step` before the insertion loop.

- [ ] **Step 3: Add test** — insert 100 blocks between the same two blocks, verify no failure and correct order.

- [ ] **Step 4: Run tests and commit**

```bash
cargo test 2>&1
git add src/db/block.rs tests/integration.rs
git commit -m "fix: renumber page blocks when float precision exhausted"
```

---

### Task 5: Single UPDATE — merge multi-field block updates

**Files:**
- Modify: `src/db/block.rs:195-237` (update_block body)

- [ ] **Step 1: Replace three separate UPDATEs with one dynamic UPDATE**

```rust
let mut set_clauses: Vec<String> = Vec::new();
let mut params: Vec<String> = Vec::new();

if block_type.is_some() {
    set_clauses.push("block_type = ?".into());
}
if content_json.is_some() {
    set_clauses.push("content_json = ?".into());
}
if properties_json.is_some() {
    set_clauses.push("properties_json = ?".into());
}

if set_clauses.is_empty() {
    // No-op touch: just bump revision
    sqlx::query(
        "UPDATE blocks SET revision = revision + 1, updated_at = ? WHERE id = ? AND deleted = 0"
    )
    .bind(&timestamp)
    .bind(block_id)
    .execute(&mut *tx).await?;
} else {
    set_clauses.push("revision = revision + 1".into());
    set_clauses.push("updated_at = ?".into());
    let sql = format!("UPDATE blocks SET {} WHERE id = ? AND deleted = 0", set_clauses.join(", "));
    // Build query with dynamic binds...
    // Use QueryBuilder for safety
    let mut qb = sqlx::QueryBuilder::new("UPDATE blocks SET ");
    let mut separated = qb.separated(", ");
    if let Some(bt) = block_type { separated.push("block_type = ").push_bind_unseparated(bt); }
    if let Some(cj) = content_json { separated.push("content_json = ").push_bind_unseparated(cj); }
    if let Some(pj) = properties_json { separated.push("properties_json = ").push_bind_unseparated(pj); }
    separated.push("revision = revision + 1");
    separated.push("updated_at = ").push_bind_unseparated(&timestamp);
    qb.push(" WHERE id = ").push_bind(block_id).push(" AND deleted = 0");
    qb.build().execute(&mut *tx).await?;
}
```

- [ ] **Step 2: Verify revision increments by exactly 1** when updating all three fields simultaneously.

- [ ] **Step 3: Run tests and commit**

```bash
cargo test 2>&1
git add src/db/block.rs
git commit -m "fix: use single UPDATE statement for block modifications"
```

---

### Task 6: Error masking — hide raw SQL errors from clients

**Files:**
- Modify: `src/db/block.rs:17-24` (get_block)
- Modify: `src/db/block.rs:357-367` (get_block_tx)
- Modify: `src/db/block.rs:371-382` (get_order_tx)

- [ ] **Step 1: Map `sqlx::Error::RowNotFound` to `AppError::NotFound`**

In `get_block`, wrap the query:
```rust
pub async fn get_block(pool: &SqlitePool, block_id: &str) -> Result<Block> {
    sqlx::query_as::<_, Block>(
        "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE id = ? AND deleted = 0",
    )
    .bind(block_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}
```

Same pattern for `get_block_tx` (return `Option<Block>` or use `.ok_or(AppError::NotFound)`) and `get_order_tx`.

- [ ] **Step 2: Also fix `get_page`** in `db/page.rs` to use `fetch_optional` + `.ok_or(AppError::NotFound)` instead of `fetch_one`.

- [ ] **Step 3: Verify error message is clean**

```bash
cargo test 2>&1
# Test manually:
curl -s http://127.0.0.1:14318/pages/nonexistent -H "Authorization: Bearer $TOKEN"
```
Expected: `{"error":"not found"}` instead of SQL row error.

- [ ] **Step 4: Commit**

```bash
git add src/db/block.rs src/db/page.rs
git commit -m "fix: return clean NotFound errors instead of raw SQL messages"
```

---

### Task 7: Input validation — block_type and content_json

**Files:**
- Create: `src/db/validate.rs`
- Modify: `src/api/blocks.rs` (BlockInput deserialization)

- [ ] **Step 1: Create `src/db/validate.rs`**

```rust
use documosa_core::rich_text::RichTextToken;
use crate::error::{AppError, Result};

const VALID_BLOCK_TYPES: &[&str] = &[
    "paragraph", "heading_1", "heading_2", "heading_3",
    "bulleted_list_item", "numbered_list_item", "to_do", "toggle",
    "code", "equation", "quote", "callout", "divider",
    "image", "table", "table_row", "column_list", "column",
    "child_page", "breadcrumb",
];

pub(crate) fn validate_block_input(input: &super::block::BlockInput) -> Result<()> {
    if !VALID_BLOCK_TYPES.contains(&input.block_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "invalid block_type: {}. Must be one of: {}", 
            input.block_type,
            VALID_BLOCK_TYPES.join(", ")
        )));
    }
    // Validate content_json is valid RichText array
    let tokens: Vec<RichTextToken> = serde_json::from_str(&input.content_json)
        .map_err(|e| AppError::BadRequest(format!("invalid content_json: {e}")))?;
    if tokens.is_empty() && !matches!(input.block_type.as_str(), "divider" | "table") {
        return Err(AppError::BadRequest("content_json must be non-empty rich text array".into()));
    }
    Ok(())
}
```

- [ ] **Step 2: Add `mod validate; pub(crate) use validate::validate_block_input;`** to `src/db/mod.rs`

- [ ] **Step 3: Call validation in `db/block.rs` `append_blocks`**

At the start of `append_blocks`, after the empty check:
```rust
for input in &blocks {
    super::validate::validate_block_input(input)?;
}
```

Also validate in `update_block` when `block_type` or `content_json` is being changed.

- [ ] **Step 4: Add test for invalid block type rejection**

- [ ] **Step 5: Run tests and commit**

```bash
cargo test 2>&1
git add src/db/validate.rs src/db/mod.rs src/db/block.rs tests/integration.rs
git commit -m "feat: validate block_type and content_json on input"
```

---

### Task 8: Notion compat — `/v1` prefix + `object` field + cursor + error format

**Files:**
- Modify: `src/api/mod.rs` — nest under `/v1`
- Modify: `src/api/pages.rs` — wrap responses with `object`
- Modify: `src/api/blocks.rs` — wrap responses, encode cursor
- Modify: `src/error.rs` — Notion error format
- Modify: `src/mcp.rs` — update path prefix
- Modify: `src/mmdash_api.rs` — keep existing paths
- Modify: `tests/integration.rs` — update paths

- [ ] **Step 1: Nest API under `/v1` in `src/api/mod.rs`**

```rust
pub fn router() -> Router<AppState> {
    let v1 = Router::new()
        .merge(pages::router())
        .merge(blocks::router())
        .merge(comments::router())
        .merge(suggestions::router())
        .merge(history::router())
        .merge(export::router())
        .merge(ws::router());
    Router::new()
        .nest("/v1", v1)
        .merge(mmdash_api::router())
}
```

- [ ] **Step 2: Add `object` field helper**

Create a response wrapper in `src/api/mod.rs`:
```rust
use serde::Serialize;
use serde_json::{Value, json};

fn object_response(object_type: &str, body: impl Serialize) -> Value {
    let mut v = serde_json::to_value(body).unwrap_or(Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.insert("object".into(), json!(object_type));
    }
    v
}
```

Use it in handlers: `Json(object_response("page", page))`, `Json(object_response("list", results))`, `Json(object_response("block", block))`.

- [ ] **Step 3: Encode cursor as opaque string**

In `src/api/blocks.rs` `list_children` handler, change cursor from `f64` to base64 string:
```rust
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
let cursor_str = next_cursor.map(|o| BASE64.encode(format!("{}:{}", page_id_or_parent_id, o)));
```
(Add `base64` crate to Cargo.toml or just use `hex` encoding.)

- [ ] **Step 4: Notion error format in `src/error.rs`**

```rust
#[derive(Serialize)]
struct ErrorBody {
    object: String,       // "error"
    status: u16,
    code: String,
    message: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "validation_error", Some(msg.clone())),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, "restricted_resource", Some(msg.clone())),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, "conflict_error", Some(msg.clone())),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", None),
            AppError::NotFound => (StatusCode::NOT_FOUND, "object_not_found", None),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error", None),
        };
        let status_code = u16::from(status);
        let body = Json(ErrorBody { object: "error".into(), status: status_code, code: code.into(), message });
        (status, body).into_response()
    }
}
```

- [ ] **Step 5: Update all test paths and expectations**

In `tests/integration.rs`, change all URLs from `/pages` to `/v1/pages`, `/blocks` to `/v1/blocks`, etc. Update error body assertions to check for new Notion format.

- [ ] **Step 6: Run all tests**

```bash
cargo test 2>&1
```

Fix until all pass.

- [ ] **Step 7: Commit**

```bash
git add src/api/ src/error.rs src/mcp.rs tests/integration.rs Cargo.toml
git commit -m "feat: align API with Notion protocol (v1 prefix, object field, cursor, error format)"
```

---

### Task 9: Final verification

- [ ] **Step 1: Full test suite**

```bash
cargo test 2>&1
```

- [ ] **Step 2: Binary build**

```bash
cargo build 2>&1
```

- [ ] **Step 3: Quick E2E smoke**

```bash
JWT_SECRET=test cargo run -- serve --port 14319 --data-dir /tmp/doc-v2 &
TOKEN=$(python3 -c "import jwt,time;print(jwt.encode({'sub':'u','name':'T','exp':int(time.time())+3600},'test',algorithm='HS256'))")
# Create page
curl -s -X POST http://127.0.0.1:14319/v1/pages -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" -d '{"title":"test"}' | python3 -c "import sys,json; d=json.load(sys.stdin); assert d['object']=='page', 'missing object'; print('OK: page create')"
# Verify auth
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:14319/v1/pages
kill %1
```
Expected: 401 for unauthenticated, "OK" for page create.

- [ ] **Step 4: Commit any remaining fixes**

```bash
git add -A && git commit -m "chore: final verification fixes"
```
