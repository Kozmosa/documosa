# Documosa Notion API 100% Compatibility — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close all remaining gaps to reach 100% Notion API wire-protocol compatibility (7 fixes).

**Architecture:** 7 independent, small-scope fixes touching 6 files. Each fix is self-contained and can be committed separately.

**Tech Stack:** Rust 2024, axum, serde_json, base64.

---

### Task 1: next_cursor null when has_more=false

**Files:**
- Modify: `src/api/blocks.rs` — `list_children` handler

- [ ] **Step 1: Fix next_cursor logic**

In the `list_children` handler, after building `ListChildrenResponse`, set `next_cursor` to `None` when `has_more` is false:

```rust
let next_cursor = if has_more { next_cursor.map(|o| BASE64.encode(o.to_string())) } else { None };
```

- [ ] **Step 2: Run tests and commit**

```bash
cargo test 2>&1
git add src/api/blocks.rs
git commit -m "fix: set next_cursor to null when has_more is false"
```

---

### Task 2: update_block/delete_block return Block instead of PageSnapshot

**Files:**
- Modify: `src/db/block.rs` — split `update_block` and `delete_block` into tx+commit variants that return Block
- Modify: `src/api/blocks.rs` — use new return type

- [ ] **Step 1: Create `update_block_inner` and `delete_block_inner` in `src/db/block.rs`**

Extract the core logic (everything inside the transaction) into `pub(crate)` functions that return just the updated/deleted `Block`:

```rust
pub(crate) async fn update_block_tx(tx: &mut Transaction<'_, Sqlite>, actor: &Identity, block_id: &str, block_type: Option<&str>, content_json: Option<&str>, properties_json: Option<&str>) -> Result<Block> {
    let block = get_block_tx(tx, block_id).await?;
    let page_id = block.page_id.clone();
    let timestamp = now();
    // ... same QueryBuilder logic ...
    touch_page_tx(tx, &page_id).await?;
    audit_tx(tx, &page_id, actor, "block.updated", json!({...})).await?;
    // Re-read block to get updated state
    get_block_tx(tx, block_id).await
}
```

Then `update_block` calls `update_block_tx`, commits, and returns the Block. Same pattern for `delete_block`.

Optionally simpler: keep `update_block` as-is but after commit do `get_block(pool, block_id).await` instead of `snapshot`.

- [ ] **Step 2: Update handler in `src/api/blocks.rs`**

```rust
async fn update_block(...) -> Result<impl IntoResponse> {
    let block = db::update_block(...).await?;  // now returns Block
    state.hub.block_updated(&block.page_id, &block.id);
    Ok(Json(wrap_object("block", block)))
}

async fn delete_block(...) -> Result<impl IntoResponse> {
    let block = db::delete_block(...).await?;  // now returns Block
    state.hub.block_deleted(&block.page_id, &block.id);
    Ok(Json(wrap_object("block", block)))
}
```

- [ ] **Step 3: Update tests and commit**

```bash
cargo test 2>&1
git add src/db/block.rs src/api/blocks.rs tests/integration.rs
git commit -m "fix: return block instead of page snapshot on update/delete"
```

---

### Task 3: append_blocks return list instead of page snapshot

**Files:**
- Modify: `src/api/blocks.rs` — `append_blocks` handler
- Modify: `src/db/block.rs` — `append_blocks` return type

- [ ] **Step 1: Change `append_blocks` return type in `src/db/block.rs`**

Change signature from `Result<PageSnapshot>` to `Result<Vec<Block>>`:

```rust
pub async fn append_blocks(...) -> Result<Vec<Block>> {
    // ... existing logic ...
    tx.commit().await?;
    Ok(inserted)  // return the inserted blocks directly
}
```

- [ ] **Step 2: Update handler in `src/api/blocks.rs`**

```rust
async fn append_blocks(...) -> Result<impl IntoResponse> {
    let blocks = db::append_blocks(...).await?;
    let ids: Vec<String> = blocks.iter().map(|b| b.id.clone()).collect();
    state.hub.blocks_appended(&page_id, &ids, body.after.as_deref());
    
    let mut list = serde_json::Map::new();
    list.insert("object".into(), json!("list"));
    list.insert("results".into(), serde_json::to_value(blocks)?);
    list.insert("next_cursor".into(), Value::Null);
    list.insert("has_more".into(), json!(false));
    Ok(Json(Value::Object(list)))
}
```

- [ ] **Step 3: Update tests and commit**

```bash
cargo test 2>&1
git add src/db/block.rs src/api/blocks.rs tests/integration.rs
git commit -m "fix: return list of blocks instead of page snapshot on append"
```

---

### Task 4: Nested items get object field

**Files:**
- Modify: `documosa-core/src/models.rs` — add `object` field to Block, Comment, CommentReply, Suggestion
- Modify: `src/api/mod.rs` — ensure serialization

- [ ] **Step 1: Add `object` field to core types**

Add to `Block`:
```rust
#[serde(default = "default_block_object")]
pub object: String,
```
With helper:
```rust
fn default_block_object() -> String { "block".into() }
```

Same for `Comment`:
```rust
#[serde(default = "default_comment_object")]
pub object: String,
// default returns "comment"
```

And `CommentReply`: `object = "comment_reply"`.  
And `Suggestion`: `object = "suggestion"`.

- [ ] **Step 2: Set object fields in db constructors**

In `insert_block_tx`: add `object: "block".into()` to `Block { ... }`.  
In `create_comment`: add `object: "comment".into()`.

- [ ] **Step 3: Update test assertions that check block/comment JSON** to include `"object": "block"`.

- [ ] **Step 4: Run tests and commit**

```bash
cargo test 2>&1
git add documosa-core/src/models.rs src/db/block.rs src/db/comment.rs tests/integration.rs
git commit -m "feat: add object field to nested block/comment items in list responses"
```

---

### Task 5: Expand block type whitelist

**Files:**
- Modify: `src/db/validate.rs`

- [ ] **Step 1: Add missing block types to `VALID_BLOCK_TYPES`**

```rust
const VALID_BLOCK_TYPES: &[&str] = &[
    "paragraph", "heading_1", "heading_2", "heading_3",
    "bulleted_list_item", "numbered_list_item", "to_do", "toggle",
    "code", "equation", "quote", "callout", "divider",
    "image", "table", "table_row", "column_list", "column",
    "child_page",
    // New:
    "bookmark", "embed", "link_preview", "link_to_page",
    "video", "pdf", "file", "audio",
    "synced_block", "template", "breadcrumb", "child_database",
];
```

Total: 31 types.

- [ ] **Step 2: Run tests and commit**

```bash
cargo test 2>&1
git add src/db/validate.rs
git commit -m "feat: expand block type whitelist to cover all Notion types"
```

---

### Task 6: RichText text.link format

**Files:**
- Modify: `documosa-core/src/rich_text.rs` — change `link` type
- Modify: `documosa-core/src/models.rs` — ensure serialization is correct
- Modify: `web/src/lib/converter.ts` — update TypeScript type for link

- [ ] **Step 1: Add `LinkObject` struct to `documosa-core/src/rich_text.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkObject {
    pub r#type: String,  // always "url"
    pub url: String,
}
```

- [ ] **Step 2: Change `TextContent.link` field**

From:
```rust
pub link: Option<String>,
```
To:
```rust
pub link: Option<LinkObject>,
```

- [ ] **Step 3: Update frontend converter in `web/src/lib/converter.ts`**

```typescript
interface LinkObject {
  type: string
  url: string
}

// In proseMirrorTextToRichText:
href: node.marks?.find(m => m.type === 'link')?.attrs?.href as string | null,
// becomes:
text: { content: node.text || '', link: href ? { type: 'url', url: href } : null },
```

- [ ] **Step 4: Update any test fixtures that construct TextContent**

- [ ] **Step 5: Run tests and frontend build**

```bash
cargo test 2>&1
cd web && npx tsc --noEmit 2>&1 && npm run build 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add documosa-core/src/rich_text.rs web/src/lib/converter.ts tests/integration.rs
git commit -m "fix: use Notion LinkObject format for text.link"
```

---

### Task 7: Notion-Version response header

**Files:**
- Modify: `src/api/mod.rs` — add middleware layer
- Modify: `src/api/ws.rs` — add header to WS upgrade response

- [ ] **Step 1: Add header middleware in `src/api/mod.rs`**

```rust
use axum::http::{HeaderName, HeaderValue};
use tower_http::set_header::SetResponseHeaderLayer;

pub fn router() -> Router<AppState> {
    let v1 = Router::new()
        .merge(pages::router())
        .merge(blocks::router())
        .merge(comments::router())
        .merge(suggestions::router())
        .merge(history::router())
        .merge(export::router())
        .merge(ws::router())
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("notion-version"),
            HeaderValue::from_static("2022-06-28"),
        ));
    Router::new()
        .nest("/v1", v1)
        .merge(mmdash_api::router())
}
```

Note: `SetResponseHeaderLayer` is from `tower-http` which is already a dependency. No new crate needed.

- [ ] **Step 2: Run tests and commit**

```bash
cargo test 2>&1
cargo build 2>&1
git add src/api/mod.rs
git commit -m "feat: add Notion-Version response header to all v1 endpoints"
```

---

### Task 8: Final verification

- [ ] **Step 1: Full test suite**

```bash
cargo test 2>&1
```

- [ ] **Step 2: Binary build**

```bash
cargo build 2>&1
```

- [ ] **Step 3: Frontend build**

```bash
cd web && npm run build 2>&1
```

- [ ] **Step 4: Quick E2E smoke**

```bash
JWT_SECRET=test cargo run -- serve --port 14330 --data-dir /tmp/doc-100 &
TOKEN=$(python3 -c "import jwt,time; print(jwt.encode({'sub':'u','name':'T','exp':int(time.time())+3600}, 'test', algorithm='HS256'))")

# Verify cursor null when no more pages
PAGE=$(curl -s -X POST http://127.0.0.1:14330/v1/pages -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" -d '{"title":"test"}')
PID=$(echo $PAGE | python3 -c "import sys,json; print(json.load(sys.stdin)['page']['id'])")
CHILDREN=$(curl -s "http://127.0.0.1:14330/v1/blocks/$PID/children" -H "Authorization: Bearer $TOKEN")
echo $CHILDREN | python3 -c "import sys,json; d=json.load(sys.stdin); assert d['next_cursor'] is None, 'next_cursor not null'; print('OK: next_cursor null')"

# Verify update_block returns block
curl -s -X PATCH "http://127.0.0.1:14330/v1/pages/$PID/children" -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" -d '{"children":[{"block_type":"paragraph","content_json":"[{\"type\":\"text\",\"text\":{\"content\":\"x\"},\"plain_text\":\"x\"}]"}]}' > /dev/null
BID=$(curl -s "http://127.0.0.1:14330/v1/blocks/$PID/children" -H "Authorization: Bearer $TOKEN" | python3 -c "import sys,json; print(json.load(sys.stdin)['results'][0]['id'])")
UPDATED=$(curl -s -X PATCH "http://127.0.0.1:14330/v1/blocks/$BID" -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" -d '{"block_type":"heading_1"}')
echo $UPDATED | python3 -c "import sys,json; d=json.load(sys.stdin); assert d['object']=='block', f'Expected block, got {d.get(\"object\")}'; print('OK: update returns block')"

# Verify Notion-Version header
HEADERS=$(curl -s -D - -o /dev/null http://127.0.0.1:14330/v1/pages -H "Authorization: Bearer $TOKEN")
echo "$HEADERS" | grep -i notion-version

kill %1
```

- [ ] **Step 5: Commit any remaining fixes**

```bash
git add -A && git commit -m "chore: final verification for 100% Notion compat"
```
