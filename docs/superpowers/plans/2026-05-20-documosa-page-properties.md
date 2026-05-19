# Documosa Page Properties Notion Alignment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Align Page title storage and API with Notion properties format: `title` becomes rich_text stored in `properties.title.title[]`, GET/POST/PATCH all accept and return Notion format.

**Architecture:** `Page.title: String` → `Page.title_json: String` (serde_json-serialized `Vec<RichTextToken>`). API layer handles Notion-compatible properties wrapper. Backward compat with old `"title": "string"` via API-level coercion.

**Tech Stack:** Rust 2024, serde_json, axum.

---

### Task 1: Change Page model — title String → title_json

**Files:**
- Modify: `documosa-core/src/models.rs`
- Modify: `src/db/page.rs`
- Modify: `src/db/mod.rs` (migration)

- [ ] **Step 1: Change `Page.title` to `Page.title_json` in core models**

```rust
pub struct Page {
    pub id: String,
    pub title_json: String,  // was: title: String
    pub properties_json: String,
    pub created_at: String,
    pub updated_at: String,
}
```

- [ ] **Step 2: Add helper to extract plain text title**

In `documosa-core/src/models.rs`:
```rust
impl Page {
    pub fn plain_title(&self) -> String {
        let tokens: Vec<serde_json::Value> = serde_json::from_str(&self.title_json).unwrap_or_default();
        tokens.iter()
            .filter_map(|t| t.get("plain_text").and_then(|v| v.as_str()))
            .collect()
    }
}
```

- [ ] **Step 3: Update migration in `src/db/mod.rs`**

Change the `pages` table DDL:
```sql
CREATE TABLE IF NOT EXISTS pages (id TEXT PRIMARY KEY, title_json TEXT NOT NULL DEFAULT '[]', properties_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
```

Add migration for existing DBs:
```rust
// Check if old title column exists and migrate
let has_title: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info('pages') WHERE name = 'title'")
    .fetch_one(pool).await?;
if has_title.0 > 0 && has_title_json.0 == 0 {
    pool.execute("ALTER TABLE pages ADD COLUMN title_json TEXT NOT NULL DEFAULT '[]'").await?;
    // Convert existing title strings to rich_text format
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, title FROM pages")
        .fetch_all(pool).await?;
    for (id, title) in rows {
        let rt = json!([{"type":"text","text":{"content":title},"plain_text":title}]);
        sqlx::query("UPDATE pages SET title_json = ? WHERE id = ?")
            .bind(serde_json::to_string(&rt)?).bind(id).execute(pool).await?;
    }
    // SQLite doesn't support DROP COLUMN well, keep old title column but unused
}
```

- [ ] **Step 4: Update `src/db/page.rs`** — all references to `page.title` → `page.plain_title()` (for display). `create_page` stores `title_json` from input (rich_text JSON). `update_page_title` updates `title_json`.

- [ ] **Step 5: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add documosa-core/src/models.rs src/db/page.rs src/db/mod.rs
git commit -m "feat: change Page.title to rich_text-based title_json for Notion properties alignment"
```

---

### Task 2: Update API handlers — Notion properties format

**Files:**
- Modify: `src/api/pages.rs`
- Modify: `src/api/export.rs`

- [ ] **Step 1: Update `create_page` handler**

Accept Notion format:
```json
{"properties": {"title": {"title": [{"text": {"content": "My Page"}}]}}}
```
Also backward-compat with:
```json
{"title": "My Page"}
```

Handler:
```rust
async fn create_page(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse> {
    let title_rich_text: Value = body.get("properties")
        .and_then(|p| p.get("title"))
        .and_then(|t| t.get("title"))
        .cloned()
        .unwrap_or_else(|| {
            // Backward compat: title string → rich_text
            let title_str = body.get("title").and_then(|v| v.as_str()).unwrap_or("");
            json!([{"type":"text","text":{"content":title_str},"plain_text":title_str}])
        });
    
    let title_json = serde_json::to_string(&title_rich_text).unwrap_or_default();
    let snap = db::create_page(&state.pool, &actor, title_json, String::new()).await?;
    state.hub.page_created(&snap.page.id);
    Ok((StatusCode::CREATED, Json(page_to_notion_response(&snap))))
}
```

- [ ] **Step 2: Add `page_to_notion_response` helper**

```rust
fn page_to_notion_response(page: &Page) -> Value {
    let title_tokens: Value = serde_json::from_str(&page.title_json).unwrap_or(json!([]));
    json!({
        "object": "page",
        "id": page.id,
        "created_time": page.created_at,
        "last_edited_time": page.updated_at,
        "properties": {
            "title": {
                "id": "title",
                "type": "title",
                "title": title_tokens,
            }
        },
        "properties_json": page.properties_json,
    })
}
```

- [ ] **Step 3: Update `get_page` handler** — use `page_to_notion_response`

- [ ] **Step 4: Update `update_page` handler**

Accept `PATCH /v1/pages/{id}` with:
```json
{"properties": {"title": {"title": [{"text": {"content": "New Title"}}]}}}
```

```rust
async fn update_page(...) -> Result<impl IntoResponse> {
    if let Some(properties) = body.get("properties") {
        if let Some(title_prop) = properties.get("title") {
            if let Some(title_tokens) = title_prop.get("title") {
                let title_json = serde_json::to_string(title_tokens).unwrap_or_default();
                db::update_page_title(&state.pool, &actor, &page_id, &title_json).await?;
                state.hub.title_updated(&page_id, &Page::plain_title_from_json(&title_json));
            }
        }
    }
    let page = db::get_page(&state.pool, &page_id).await?;
    Ok(Json(page_to_notion_response(&page)))
}
```

Also backward-compat with `{"title": "New Title"}`.

- [ ] **Step 5: Update `get_snapshot`** — wrap Page in Notion format

- [ ] **Step 6: Update `export_markdown`** — use `page.plain_title()` for the export

- [ ] **Step 7: Update `update_page_title` in `src/db/page.rs`** — change parameter from `title: &str` to `title_json: &str`

- [ ] **Step 8: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 9: Commit**

```bash
git add src/api/pages.rs src/api/export.rs src/db/page.rs
git commit -m "feat: update page API handlers to use Notion properties format"
```

---

### Task 3: Update MMDash adapter and CLI

**Files:**
- Modify: `src/mmdash_api.rs`
- Modify: `src/cli.rs`

- [ ] **Step 1: Update `src/mmdash_api.rs`**

The MMDash adapter accepts `{title: "string"}` and returns `{page_id, title, created_at, updated_at}`. Keep the simple format for mmdash compat — internally convert title string to rich_text before calling db.

```rust
// In create_document:
let title_rt = json!([{"type":"text","text":{"content":body.title},"plain_text":body.title}]);
let title_json = serde_json::to_string(&title_rt).unwrap_or_default();
let snap = db::create_page(&state.pool, &actor, title_json, ...).await?;
```

- [ ] **Step 2: Update `src/cli.rs`** — the `document create` command sends `"title": "..."` as a string. Keep this simple API — the server creates rich_text from the string via backward compat.

- [ ] **Step 3: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add src/mmdash_api.rs src/cli.rs
git commit -m "fix: update MMDash adapter and CLI for title_json"
```

---

### Task 4: Update frontend and tests

**Files:**
- Modify: `web/src/App.tsx`
- Modify: `web/src/lib/api.ts`
- Modify: `tests/integration.rs`

- [ ] **Step 1: Update `web/src/lib/api.ts` types**

```typescript
interface Page {
    object: string
    id: string
    created_time: string
    last_edited_time: string
    properties: {
        title: {
            id: string
            type: string
            title: RichTextToken[]
        }
    }
    properties_json: string
}

function pageTitle(page: Page): string {
    return page.properties?.title?.title?.map(t => t.plain_text).join('') || ''
}
```

- [ ] **Step 2: Update `web/src/App.tsx`**

Replace `snapshot.page.title` / `page.title` with `pageTitle(page)` helper.

- [ ] **Step 3: Update `tests/integration.rs`**

- Update test assertions for new response format
- Test backward compat: `POST /v1/pages` with `{"title": "string"}` still works
- Test Notion format: `POST /v1/pages` with `{"properties":{"title":{"title":[{"text":{"content":"Notion"}}]}}}`
- Test `PATCH /v1/pages/{id}` with `{"properties":{"title":{"title":[{"text":{"content":"Updated"}}]}}}`

- [ ] **Step 4: Verify all tests and builds**

```bash
cargo test 2>&1
cd web && npx tsc --noEmit 2>&1 && npm run build 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add tests/integration.rs web/src/App.tsx web/src/lib/api.ts
git commit -m "feat: update frontend and tests for Notion page properties format"
```

---

### Task 5: Final verification

- [ ] **Step 1: All tests**

```bash
cargo test 2>&1
```

- [ ] **Step 2: Binaries**

```bash
cargo build 2>&1
cd web && npm run build 2>&1
```

- [ ] **Step 3: E2E smoke**

```bash
JWT_SECRET=test cargo run -- serve --port 14350 --data-dir /tmp/doc-prop &
TOKEN=$(...)
# Test Notion format create
curl -s -X POST http://127.0.0.1:14350/v1/pages -H "Authorization: Bearer $TOKEN" \
  -d '{"properties":{"title":{"title":[{"text":{"content":"Notion Page"}}]}}}' \
  | python3 -c "import sys,json; d=json.load(sys.stdin); ..."
# Test backward compat
curl -s -X POST http://127.0.0.1:14350/v1/pages -H "Authorization: Bearer $TOKEN" \
  -d '{"title":"Legacy Page"}' \
  | python3 -c "..."
# Test GET returns properties format
# Test PATCH updates title
kill %1
```

- [ ] **Step 4: Commit final fixes**

```bash
git add -A && git commit -m "chore: final verification for page properties Notion alignment"
```
