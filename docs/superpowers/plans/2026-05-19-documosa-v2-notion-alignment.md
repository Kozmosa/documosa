# Documosa v2.0 Notion Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite documosa from line-based Markdown storage to block-based Notion-aligned storage with Tiptap editor, replacing all APIs and the frontend.

**Architecture:** documosa-core gets new Block/Page/RichText models. documosa gets rewritten db layer (SQLite block tables), new REST API (`/v1/*`), updated MCP tools (dual-format), and new Tiptap React frontend. Old line-based code, API, and CherryEditor are fully deleted.

**Tech Stack:** Rust 2024, axum 0.8, sqlx 0.8, tokio, Tiptap React, shadcn/ui, KaTeX, lowlight.

---

### Task 1: Rewrite documosa-core models with Block/Page/RichText

**Files:**
- Rewrite: `documosa-core/src/models.rs`
- Create: `documosa-core/src/rich_text.rs`
- Modify: `documosa-core/src/lib.rs`
- Modify: `documosa-core/Cargo.toml`

- [ ] **Step 1: Add optional sqlx feature to Cargo.toml**

```toml
[features]
default = []
sqlx = ["dep:sqlx"]
```

- [ ] **Step 2: Create `documosa-core/src/rich_text.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RichTextToken {
    pub r#type: String,
    pub text: Option<TextContent>,
    pub annotations: Option<Annotations>,
    pub plain_text: String,
    pub href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextContent {
    pub content: String,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Annotations {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub code: bool,
    pub color: String,
}

impl Default for Annotations {
    fn default() -> Self {
        Self {
            bold: false, italic: false, strikethrough: false,
            underline: false, code: false, color: "default".into(),
        }
    }
}

impl Default for RichTextToken {
    fn default() -> Self {
        Self {
            r#type: "text".into(),
            text: Some(TextContent { content: String::new(), link: None }),
            annotations: Some(Annotations::default()),
            plain_text: String::new(),
            href: None,
        }
    }
}
```

- [ ] **Step 3: Rewrite `documosa-core/src/models.rs`** — replace all Line/Document types with Block/Page types:

```rust
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "sqlx")]
use sqlx::FromRow;

use crate::error::ProtocolError;

pub fn now() -> String { Utc::now().to_rfc3339() }
pub fn new_id() -> String { Uuid::new_v4().to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct Page {
    pub id: String,
    pub title: String,
    pub properties_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct Block {
    pub id: String,
    pub page_id: String,
    pub parent_id: Option<String>,
    pub order_index: f64,
    pub block_type: String,
    pub content_json: String,
    pub properties_json: String,
    pub revision: i64,
    pub deleted: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct BlockLock {
    pub block_id: String,
    pub page_id: String,
    pub owner_client_id: String,
    pub owner_nickname: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct Comment {
    pub id: String,
    pub page_id: String,
    pub target_block_id: String,
    pub start_column: Option<i64>,
    pub end_column: Option<i64>,
    pub author_client_id: String,
    pub author_nickname: String,
    pub role_mode: String,
    pub body: String,
    pub resolved: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct CommentReply {
    pub id: String,
    pub comment_id: String,
    pub author_client_id: String,
    pub author_nickname: String,
    pub role_mode: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct Suggestion {
    pub id: String,
    pub page_id: String,
    pub kind: String,
    pub target_block_id: Option<String>,
    pub content_json: String,
    pub base_revisions_json: String,
    pub state: String,
    pub author_client_id: String,
    pub author_nickname: String,
    pub role_mode: String,
    pub created_at: String,
    pub decided_by_client_id: Option<String>,
    pub decided_by_nickname: Option<String>,
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct AuditEvent {
    pub id: String,
    pub page_id: String,
    pub actor_client_id: String,
    pub actor_nickname: String,
    pub role_mode: String,
    pub event_type: String,
    pub details_json: String,
    pub created_at: String,
    pub note_body: Option<String>,
    pub note_updated_by_nickname: Option<String>,
    pub note_updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HistoryCategory {
    DocumentComment,
    All,
    Content,
    Comment,
    Suggestion,
    System,
}

impl HistoryCategory {
    pub fn parse(value: &str) -> Result<Self, ProtocolError> {
        match value {
            "document-comment" => Ok(Self::DocumentComment),
            "all" => Ok(Self::All),
            "content" => Ok(Self::Content),
            "comment" => Ok(Self::Comment),
            "suggestion" => Ok(Self::Suggestion),
            "system" => Ok(Self::System),
            _ => Err(ProtocolError::BadRequest("invalid history category".into())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HistoryListOptions {
    pub category: HistoryCategory,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSnapshot {
    pub page: Page,
    pub blocks: Vec<Block>,
    pub comments: Vec<Comment>,
    pub replies: Vec<CommentReply>,
    pub suggestions: Vec<Suggestion>,
    pub locks: Vec<BlockLock>,
    pub audit_events: Vec<AuditEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryDiff {
    pub from_event: AuditEvent,
    pub to_event: AuditEvent,
    pub from_content: String,
    pub to_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseRevision {
    pub block_id: String,
    pub revision: i64,
}
```

- [ ] **Step 4: Update `documosa-core/src/lib.rs`**

```rust
pub mod error;
pub mod identity;
pub mod models;
pub mod operations;
pub mod permissions;
pub mod rich_text;
```

- [ ] **Step 5: Ensure documosa-core compiles**

```bash
cd /home/xuyang/code/kozbox/apps/documosa/documosa-core && cargo check 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add documosa-core/ && git add documosa-core/src/rich_text.rs
git commit -m "feat: rewrite documosa-core models for v2.0 Block/Page/RichText"
```

---

### Task 2: Rewrite documosa-core operations and permissions

**Files:**
- Rewrite: `documosa-core/src/operations.rs`
- Rewrite: `documosa-core/src/permissions.rs`

- [ ] **Step 1: Rewrite `documosa-core/src/operations.rs`**

```rust
use serde::{Deserialize, Serialize};
use crate::identity::Identity;
use crate::models::*;

#[allow(async_fn_in_trait)]
pub trait DocumosaOps {
    // page
    async fn create_page(&self, actor: &Identity, title: String, content_blocks: Vec<BlockInput>)
        -> Result<PageSnapshot, crate::error::ProtocolError>;
    async fn get_page(&self, page_id: &str) -> Result<PageSnapshot, crate::error::ProtocolError>;
    async fn update_page_title(&self, actor: &Identity, page_id: &str, title: &str)
        -> Result<(), crate::error::ProtocolError>;
    async fn export_page_markdown(&self, page_id: &str) -> Result<String, crate::error::ProtocolError>;

    // blocks
    async fn get_block(&self, block_id: &str) -> Result<Block, crate::error::ProtocolError>;
    async fn list_block_children(&self, block_id: &str, cursor: Option<f64>, page_size: i64)
        -> Result<(Vec<Block>, Option<f64>), crate::error::ProtocolError>;
    async fn append_blocks(&self, actor: &Identity, page_id: &str,
        blocks: Vec<BlockInput>, after: Option<&str>) -> Result<Vec<Block>, crate::error::ProtocolError>;
    async fn update_block(&self, actor: &Identity, block_id: &str,
        block_type: Option<&str>, content_json: Option<&str>,
        properties_json: Option<&str>) -> Result<Block, crate::error::ProtocolError>;
    async fn delete_block(&self, actor: &Identity, block_id: &str)
        -> Result<Block, crate::error::ProtocolError>;

    // locks
    async fn lock_blocks(&self, actor: &Identity, page_id: &str,
        block_ids: Vec<&str>, ttl_seconds: i64) -> Result<Vec<BlockLock>, crate::error::ProtocolError>;
    async fn release_locks(&self, actor: &Identity, page_id: &str,
        block_ids: Vec<&str>) -> Result<(), crate::error::ProtocolError>;

    // comments
    async fn create_comment(&self, actor: &Identity, page_id: &str, block_id: &str,
        start_column: Option<i64>, end_column: Option<i64>, body: &str)
        -> Result<Comment, crate::error::ProtocolError>;
    async fn reply_comment(&self, actor: &Identity, page_id: &str, comment_id: &str, body: &str)
        -> Result<CommentReply, crate::error::ProtocolError>;
    async fn update_comment(&self, actor: &Identity, page_id: &str, comment_id: &str, body: &str)
        -> Result<Comment, crate::error::ProtocolError>;
    async fn resolve_comment(&self, actor: &Identity, page_id: &str, comment_id: &str)
        -> Result<Comment, crate::error::ProtocolError>;

    // suggestions
    async fn create_suggestion(&self, actor: &Identity, page_id: &str, kind: SuggestionKind,
        target_block_id: Option<&str>, new_blocks: Vec<BlockInput>,
        base_revisions: Vec<BaseRevision>) -> Result<Suggestion, crate::error::ProtocolError>;
    async fn accept_suggestion(&self, actor: &Identity, page_id: &str, suggestion_id: &str)
        -> Result<Suggestion, crate::error::ProtocolError>;
    async fn reject_suggestion(&self, actor: &Identity, page_id: &str, suggestion_id: &str)
        -> Result<Suggestion, crate::error::ProtocolError>;

    // history
    async fn list_history(&self, page_id: &str, category: HistoryCategory,
        from: Option<&str>, to: Option<&str>, limit: i64)
        -> Result<Vec<AuditEvent>, crate::error::ProtocolError>;
    async fn history_diff(&self, page_id: &str, from_event_id: &str, to_event_id: &str)
        -> Result<HistoryDiff, crate::error::ProtocolError>;
    async fn set_audit_note(&self, actor: &Identity, page_id: &str,
        audit_event_id: &str, body: &str) -> Result<(), crate::error::ProtocolError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    InsertBlocks,
    ReplaceBlocks,
    DeleteBlocks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInput {
    pub block_type: String,
    pub content_json: String,
    #[serde(default)]
    pub properties_json: Option<String>,
}
```

- [ ] **Step 2: Rewrite `documosa-core/src/permissions.rs`**

```rust
use crate::identity::RoleMode;

pub const WRITER_OPS: &[&str] = &[
    "create_page",
    "get_block", "append_blocks", "update_block", "delete_block",
    "lock_blocks", "release_locks",
    "create_comment", "reply_comment", "update_comment", "resolve_comment",
    "create_suggestion", "accept_suggestion", "reject_suggestion",
    "view_history", "history_diff", "set_audit_note",
    "export_page_markdown",
];

pub const REVIEWER_OPS: &[&str] = &[
    "create_page",
    "get_block",
    "create_comment", "reply_comment", "update_comment", "resolve_comment",
    "create_suggestion",
    "view_history", "history_diff", "set_audit_note",
    "export_page_markdown",
];

pub fn is_allowed(role: RoleMode, operation: &str) -> bool {
    match role {
        RoleMode::Writer => WRITER_OPS.contains(&operation),
        RoleMode::Reviewer => REVIEWER_OPS.contains(&operation),
    }
}
```

- [ ] **Step 3: Check compilation**

```bash
cd documosa-core && cargo check 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add documosa-core/src/operations.rs documosa-core/src/permissions.rs
git commit -m "feat: rewrite core operations and permissions for block model"
```

---

### Task 3: Rewrite SQLite migration and db modules

**Files:**
- Rewrite: `src/db/mod.rs` — migration + pool, no more line references
- Create: `src/db/page.rs` — page CRUD
- Rewrite: `src/db/block.rs` — block CRUD
- Rewrite: `src/db/comment.rs` — `start_line_id/end_line_id` → `target_block_id`
- Rewrite: `src/db/suggestion.rs` — `anchor_line_id/...` → `target_block_id`
- Rewrite: `src/db/lock.rs` — `line_id` → `block_id`
- Rewrite: `src/db/history.rs` — update references
- Rewrite: `src/db/audit.rs` — touch_document → touch_page
- Rewrite: `src/db/ops.rs` — impl new DocumosaOps
- Keep: `src/db/permission.rs` (update operation strings)
- Delete: `src/db/line.rs`, `src/db/document.rs`, `src/db/text.rs`
- Modify: `src/error.rs` — update error types if needed

- [ ] **Step 1: Rewrite `src/db/mod.rs`** — new migration with all tables from spec section 4.1, pool functions unchanged, re-exports updated:

```rust
mod page;     pub use page::*;
mod block;    pub use block::*;
mod comment;  pub use comment::*;
mod suggestion; pub use suggestion::*;
mod lock;     pub use lock::*;
mod history;  pub use history::*;
mod audit;    pub(crate) use audit::*;
mod ops;
mod permission; pub(crate) use permission::require_permission;
```

Tables: `pages`, `blocks`, `block_locks`, `comments`, `comment_replies`, `suggestions`, `audit_events`, `audit_event_notes`, `page_versions`.

- [ ] **Step 2: Create `src/db/page.rs`** — `create_page(pool, actor, title, blocks)`, `get_page(pool, page_id)`, `update_page_title(pool, actor, page_id, title)`, `snapshot(pool, page_id) → PageSnapshot`, `export_markdown(pool, page_id) → String`

- [ ] **Step 3: Create `src/db/block.rs`** — `get_block(pool, block_id)`, `list_children(pool, parent_block_id, cursor, page_size)`, `append_blocks(pool, actor, page_id, blocks, after)`, `update_block(pool, actor, block_id, block_type, content, properties)`, `delete_block(pool, actor, block_id)`, internal helpers for order_index midpoint insertion and renumbering

- [ ] **Step 4: Rewrite remaining db modules** — update `line_id`/`document_id` → `block_id`/`page_id` in comment.rs, suggestion.rs, lock.rs, history.rs, audit.rs

- [ ] **Step 5: Rewrite `src/db/ops.rs`** — `impl DocumosaOps for SqlitePool` using new block operations

- [ ] **Step 6: Update `src/db/permission.rs`** — operation name strings updated

- [ ] **Step 7: Delete old files**

```bash
rm src/db/line.rs src/db/document.rs src/db/text.rs
```

- [ ] **Step 8: Update `src/models.rs`** — re-export from core:

```rust
pub use documosa_core::models::*;
pub use documosa_core::identity::*;
pub use documosa_core::rich_text::*;
```

- [ ] **Step 9: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 10: Commit**

```bash
git add -A && git rm src/db/line.rs src/db/document.rs src/db/text.rs
git commit -m "feat: rewrite db layer for block-based storage"
```

---

### Task 4: Create new REST API — pages and blocks

**Files:**
- Create: `src/api/mod.rs`
- Create: `src/api/pages.rs`
- Create: `src/api/blocks.rs`
- Modify: `src/lib.rs` — update router assembly
- Delete: `src/api.rs` (replaced by `src/api/`)

- [ ] **Step 1: Create `src/api/mod.rs`** — assemble router:

```rust
mod pages;
mod blocks;
mod comments;
mod suggestions;
mod history;
mod export;
mod ws;

use axum::Router;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/v1", pages::router())
        .nest("/v1", blocks::router())
        .nest("/v1", comments::router())
        .nest("/v1", suggestions::router())
        .nest("/v1", history::router())
        .nest("/v1", export::router())
        .nest("/v1", ws::router())
        .merge(crate::mmdash_api::router())
}
```

- [ ] **Step 2: Create `src/api/pages.rs`** with Notion-aligned endpoints:

```rust
POST   /v1/pages        → create_page
GET    /v1/pages/{id}   → get_page_metadata
PATCH  /v1/pages/{id}   → update_page_title
GET    /v1/pages/{id}/snapshot → full snapshot
```

Each handler extracts Identity from JWT `Authorization: Bearer <token>` header using `MmdashIdentity` extractor (which we'll make the default auth for all endpoints).

- [ ] **Step 3: Create `src/api/blocks.rs`**:

```rust
GET    /v1/blocks/{block_id}            → get_block
GET    /v1/blocks/{block_id}/children   → list_children (query: page_size, start_cursor)
PATCH  /v1/blocks/{block_id}            → update_block
DELETE /v1/blocks/{block_id}            → delete_block
PATCH  /v1/pages/{page_id}/children     → append_blocks (Body: { children: [BlockInput], after?: block_id })
```

- [ ] **Step 4: Update `src/lib.rs`** — `api::router` → `api::router()`:

```rust
use crate::api;
pub async fn build_app(pool: SqlitePool, web_dir: PathBuf) -> Router {
    let state = AppState { pool, hub: EventHub::new(), web_dir: Arc::new(web_dir) };
    api::router()
        .merge(mcp::router())
        .fallback(static_files::serve)
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
```

- [ ] **Step 5: Build and fix compilation errors**

```bash
cargo check 2>&1
```

Continue iterating until compiles. The db module has all the functions — just wire them up.

- [ ] **Step 6: Commit**

```bash
git rm src/api.rs && git add src/api/ src/lib.rs
git commit -m "feat: create Notion-aligned REST API for pages and blocks"
```

---

### Task 5: Create comments, suggestions, history, export, ws API modules

**Files:**
- Create: `src/api/comments.rs`
- Create: `src/api/suggestions.rs`
- Create: `src/api/history.rs`
- Create: `src/api/export.rs`
- Create: `src/api/ws.rs`
- Modify: `src/realtime.rs` — update ServerEvent for block types

- [ ] **Step 1: Create `src/api/comments.rs`** — Notion-aligned comment endpoints (spec section 3.3)

- [ ] **Step 2: Create `src/api/suggestions.rs`** — suggestion endpoints (spec section 3.4)

- [ ] **Step 3: Create `src/api/history.rs`** — history + diff + audit note (spec section 3.5)

- [ ] **Step 4: Create `src/api/export.rs`** — `GET /v1/pages/{id}/export/md` — export to markdown using `mmdash_blocks::blocks_to_markdown`

- [ ] **Step 5: Create `src/api/ws.rs`** — WebSocket handler, JWT from query params or upgrade header

- [ ] **Step 6: Update `src/realtime.rs`** — ServerEvent replaces `LinesInserted`/`LinesReplaced`/`LinesDeleted` with `BlockInserted`/`BlockUpdated`/`BlockDeleted`. Keep other events (`CommentCreated`, `CommentResolved`, `SuggestionCreated`, `SuggestionDecided`, `PageTitleUpdated`, `LocksChanged`, `ContentChanged`, `Presence`).

- [ ] **Step 7: Check compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add src/api/ src/realtime.rs
git commit -m "feat: create comment, suggestion, history, export and WS API modules"
```

---

### Task 6: Rewrite MCP tools for block operations + dual format

**Files:**
- Rewrite: `src/mcp.rs`

- [ ] **Step 1: Rewrite MCP tools list** — replace line-based tool names with block-based ones:

```
page_create, page_get, page_snapshot, page_export_md
block_get, block_list_children, block_append, block_update, block_delete
comment_create, reply_comment, resolve_comment
suggestion_create, suggestion_accept, suggestion_reject
history_list, history_diff
```

- [ ] **Step 2: Add `content_format` parameter** — each block create/update tool accepts `content_format: "rich_text" | "markdown"`. When `markdown`, convert: `markdown → blocks` (via `mmdash_blocks`) then `blocks → rich_text`. When `rich_text` (default), use directly.

- [ ] **Step 3: Wire up MCP dispatch** — `call_tool` matches new tool names and calls corresponding db functions.

- [ ] **Step 4: Keep existing actor_from_args** with ActorKind parsing (Task 4 from Phase 1 polish — already done)

- [ ] **Step 5: Check compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add src/mcp.rs
git commit -m "feat: rewrite MCP tools for block operations with dual format support"
```

---

### Task 7: Update MMDash adapter and CLI

**Files:**
- Modify: `src/mmdash_api.rs`
- Modify: `src/mmdash_auth.rs` — generalize JWT extractor name (rename `MmdashIdentity` → `DocumosaIdentity` or keep)
- Modify: `src/mmdash_blocks.rs` — ensure `blocks_to_markdown` and `markdown_to_blocks` work with new RichText format
- Modify: `src/cli.rs` — update to new `/v1/` endpoints

- [ ] **Step 1: Update `src/mmdash_api.rs`** — update endpoints to call new db functions (`create_page`, `get_page`, `export_markdown`, etc.)

- [ ] **Step 2: Update `src/mmdash_blocks.rs`** — add `rich_text_to_plain_text()` helper for exporting blocks to markdown

- [ ] **Step 3: Update `src/cli.rs`** — update all REST paths from `/api/documents/...` to `/v1/pages/...` and `/v1/blocks/...`

- [ ] **Step 4: Check compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/mmdash_api.rs src/mmdash_blocks.rs src/cli.rs
git commit -m "feat: update MMDash adapter and CLI for v2.0 Notion API"
```

---

### Task 8: Install Tiptap and create converter

**Files:**
- Create: `web/src/lib/converter.ts` — ProseMirror ↔ RichText converter
- Create: `web/src/lib/api.ts` — updated API client
- Create: `web/src/lib/ws.ts` — WebSocket client
- Modify: `web/package.json`
- Modify: `web/src/main.tsx` — update imports

- [ ] **Step 1: Install Tiptap dependencies**

```bash
cd web
npm install @tiptap/react @tiptap/starter-kit @tiptap/extension-code-block-lowlight @tiptap/extension-table @tiptap/extension-task-list @tiptap/extension-task-item @tiptap/extension-placeholder @tiptap/pm lowlight katex
```

- [ ] **Step 2: Create `web/src/lib/converter.ts`** — two functions:

```typescript
// ProseMirror JSON → documosa Block (with rich_text content)
function proseMirrorToBlocks(doc: ProseMirrorNode[]): BlockInput[]
// documosa Blocks → ProseMirror JSON  
function blocksToProseMirror(blocks: Block[]): ProseMirrorNode
```

Map ProseMirror marks to rich_text annotations (spec section 5.4), and ProseMirror node types to block types (spec section 5.5).

- [ ] **Step 3: Create `web/src/lib/api.ts`** — typed fetch wrapper for all `/v1/` endpoints

- [ ] **Step 4: Create `web/src/lib/ws.ts`** — WebSocket client connecting to `/v1/pages/{id}/ws`

- [ ] **Step 5: Verify TypeScript compilation**

```bash
cd web && npx tsc --noEmit 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add web/
git commit -m "feat: install Tiptap and create ProseMirror/RichText converter"
```

---

### Task 9: Build Tiptap editor component

**Files:**
- Create: `web/src/TiptapEditor.tsx`
- Modify: `web/src/App.tsx` — integrate TiptapEditor
- Delete: `web/src/CherryEditor.tsx`

- [ ] **Step 1: Create `web/src/TiptapEditor.tsx`**

Key features:
- CodeMirror-style extensions list from spec section 5.3
- Notion-style UI: hover 6-dot handle, "+" add button
- Slash command menu (`/h1`, `/h2`, `/h3`, `/code`, `/eq`, `/todo`, `/quote`, `/divider`, `/table`, `/image`)
- Drag-and-drop reordering
- Indent/outdent for list nesting
- KaTeX equation rendering for custom `equation` node
- Syntax-highlighted code blocks (lowlight)
- Table support
- Props: `pageId`, `blocks`, `readOnly`, `onChange`, `onDirtyChange`, `onSelectionChange`

- [ ] **Step 2: Wire up autosave** — 2-second debounce, sends changed blocks to `/v1/blocks/{id}`

- [ ] **Step 3: Replace CherryEditor in `web/src/App.tsx`** — remove CherryEditor import, use TiptapEditor. Update all references to `snapshotText(lines)` → `blocksToProseMirror(snapshot.blocks)`.

- [ ] **Step 4: Delete `web/src/CherryEditor.tsx`**

- [ ] **Step 5: Verify build**

```bash
cd web && npm run build 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add web/ && git rm web/src/CherryEditor.tsx
git commit -m "feat: build Tiptap editor component with Notion-style UX"
```

---

### Task 10: Build comment panel and WebSocket sync

**Files:**
- Create: `web/src/CommentPanel.tsx`
- Modify: `web/src/App.tsx` — comments sidebar, WS integration

- [ ] **Step 1: Create `web/src/CommentPanel.tsx`** — render comments for selected block, reply form, resolve button

- [ ] **Step 2: Update `web/src/App.tsx`** WS handler — listen for `block_inserted`, `block_updated`, `block_deleted`, `comment_created`, etc. Apply incremental updates where possible, otherwise refresh block tree.

- [ ] **Step 3: Wire comment creation** — text selection → anchor to block → create comment via API

- [ ] **Step 4: Verify build**

```bash
cd web && npm run build 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add web/
git commit -m "feat: add comment panel and WebSocket block-level sync"
```

---

### Task 11: Rewrite integration tests

**Files:**
- Rewrite: `tests/integration.rs`

- [ ] **Step 1: Rewrite tests for new API**

Test each endpoint group:
- Page create/get/update/snapshot
- Block get/children/append/update/delete
- Comment CRUD
- Suggestion create/accept/reject
- History list/diff
- Export markdown
- Block lock heartbeat/release
- MCP tools (page_create, block_append, block_update, etc.)
- MMDash adapter endpoints
- CLI commands (updated paths)

- [ ] **Step 2: Keep existing test structure** — `actor()` helper, `pool()` helper, app builder

- [ ] **Step 3: Run tests**

```bash
cargo test 2>&1
```

Fix until all pass.

- [ ] **Step 4: Commit**

```bash
git add tests/integration.rs
git commit -m "test: rewrite integration tests for v2.0 Notion API"
```

---

### Task 12: Final verification

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

- [ ] **Step 4: Benchmarks compile**

```bash
cargo check --benches 2>&1
```

- [ ] **Step 5: Commit any fixes**

```bash
git add -A && git commit -m "chore: final verification and fixes for v2.0"
```
