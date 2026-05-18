# Documosa v1.0 打磨 — 设计文档

> 2026-05-19 | Phase 1 完成后，对 documosa 参考实现的 5 项内部打磨

---

## 1. db.rs 模块拆分

### 现状

`src/db.rs` 约 1500 行，包含所有文档/行/评论/suggestion/锁/历史/审计的 SQL 逻辑。

### 设计

按领域拆分为子模块，`src/db/mod.rs` 统一 re-export：

```
src/db/
  mod.rs        — connect, connect_memory, migrate + 所有 pub 函数 re-export
  document.rs   — create_document, list_documents, update_document_title, snapshot, export_document + 内部 ensure_document_exists
  line.rs       — insert_lines, replace_lines, delete_lines, update_content + order/renumber/insertion_bounds/line_range/active_lines 辅助
  lock.rs       — heartbeat_locks, release_locks, locks + prune/ensure_unlocked 辅助
  comment.rs    — CommentDraft, create_comment, update_comment, delete_comment, reply_comment, resolve_comment + get_comment 辅助
  suggestion.rs — create_suggestion, decide_suggestion
  history.rs    — list_history_events, put_audit_event_note, history_diff + category_filter/version_content/audit_event 辅助
  audit.rs      — audit_tx, begin_write_tx, touch_document_tx
  text.rs       — split_lines_preserve_trailing, text_summary, text_len, line_count（纯文本工具函数，无 SQL）
```

### 约束

- 所有 `pub async fn` 签名不变
- `src/lib.rs` 从 `pub mod db` 改为 `pub mod db`（自动找 `db/mod.rs`）
- 内部辅助函数用 `pub(crate)` 可见性
- `tests/integration.rs` 不受影响（通过 `documosa::db::*` 调用）
- 常量 `HISTORY_DEFAULT_LIMIT`、`HISTORY_MAX_LIMIT`、`AUDIT_NOTE_MAX_CHARS`、`LINE_ORDER_STEP` 放在 `db/mod.rs`

---

## 2. DocumosaOps trait 落地

### 现状

`documosa-core/src/operations.rs` 定义了 `DocumosaOps` trait，无实现。`db.rs` 的函数签名与 trait 方法有差异。

### 设计

在 `src/db/ops.rs` 中为 `SqlitePool` 实现 `DocumosaOps`：

```rust
impl DocumosaOps for SqlitePool {
    type Error = AppError;
    // 每个方法委托到同名 db 函数，返回值做适配
}
```

**返回值差异处理**：
- trait 方法返回 `Vec<Line>` 的，从 `DocumentSnapshot.lines` 提取
- trait 方法返回具体类型的，直接映射
- 错误通过 `From<AppError> for ProtocolError` 自动转换（已有 `#[from]`）

**调用点**：现有 `api.rs`、`mcp.rs` 继续用 `db::*` 函数（返回 full snapshot 方便前端）。trait 供外部 crate（AINRF、MMDash）通过 `use documosa_core::operations::DocumosaOps` 调用。

---

## 3. 权限体系贯通

### 现状

`db.rs` 用硬编码 `require_writer()` 检查角色，没有使用 core 里的 `is_allowed()`。

### 设计

1. 删除 `require_writer()`
2. 新增 `require_permission(actor, operation)`，内部调用 `documosa_core::permissions::is_allowed()`
3. 每个写操作的 db 函数在入口处调用 `require_permission(actor, "operation_name")`

### 操作 → 权限名映射

| db 函数 | 权限检查 |
|--------|---------|
| `update_content` | `require_permission(actor, "replace_lines")` |
| `insert_lines` | `require_permission(actor, "insert_lines")` |
| `replace_lines` | `require_permission(actor, "replace_lines")` |
| `delete_lines` | `require_permission(actor, "delete_lines")` |
| `heartbeat_locks` | `require_permission(actor, "lock_lines")` |
| `release_locks` | `require_permission(actor, "release_locks")` |
| `resolve_comment` | `require_permission(actor, "resolve_comment")` |
| `decide_suggestion` (accept) | `require_permission(actor, "accept_suggestion")` |
| `decide_suggestion` (reject) | `require_permission(actor, "reject_suggestion")` |

### 关键变化

**`resolve_comment`**：之前只有 Writer 能 resolve。现在 Reviewer 也能 resolve（已在 `REVIEWER_OPS` 中），逻辑变为：
- Reviewer 只能 resolve 自己的 comment（已有检查）
- Writer 可以 resolve 任意 comment

**`update_comment` / `delete_comment`**：已有 "reviewer can only modify their own" 逻辑，不再额外加 `require_writer`。

---

## 4. ActorKind 审计记录

### 现状

Identity 有 `actor_kind` 字段，但 `audit_tx` 不记录它。

### 设计

在 `audit_tx` 中将 `actor_kind` 序列化进 `details_json`，不改表结构：

```rust
match &actor.actor_kind {
    ActorKind::Agent { agent_id, session_ref, task_ref } => {
        details["actor_kind"] = json!("agent");
        details["agent_id"] = json!(agent_id);
        if let Some(sr) = session_ref { details["session_ref"] = json!(sr); }
        if let Some(tr) = task_ref { details["task_ref"] = json!(tr); }
    }
    ActorKind::Human => {} // 不额外写，保持 audit 体积小
}
```

现有调用点不需要改动——`mmdash_auth` 和 `identity_from_headers` 默认 `ActorKind::Human`，MCP 入口已有 agent 解析（Phase 1 完成）。

---

## 5. WebSocket 事件细化（breaking change）

### 现状

```rust
enum ServerEvent {
    Presence { document_id, users },
    DocumentChanged { document_id, topic },  // topic: "lines.inserted" 等
}
```

前端收到 `DocumentChanged` 后无差别全量刷新。

### 设计

替换为具体事件变体，删除 `DocumentChanged`：

```rust
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerEvent {
    Presence { document_id, users },
    LinesInserted { document_id, line_ids: Vec<String>, after_line_id: Option<String> },
    LinesReplaced { document_id, line_ids: Vec<String> },
    LinesDeleted { document_id, line_ids: Vec<String> },
    CommentCreated { document_id, comment_id: String },
    CommentResolved { document_id, comment_id: String },
    SuggestionCreated { document_id, suggestion_id: String },
    SuggestionDecided { document_id, suggestion_id: String, accepted: bool },
    DocumentTitleUpdated { document_id, title: String },
    LocksChanged { document_id },
}
```

`EventHub` 新增对应方法替换 `document_changed`：

```rust
impl EventHub {
    pub fn lines_inserted(&self, doc_id, line_ids, after_line_id) { ... }
    pub fn lines_replaced(&self, doc_id, line_ids) { ... }
    pub fn lines_deleted(&self, doc_id, line_ids) { ... }
    pub fn comment_created(&self, doc_id, comment_id) { ... }
    pub fn comment_resolved(&self, doc_id, comment_id) { ... }
    pub fn suggestion_created(&self, doc_id, suggestion_id) { ... }
    pub fn suggestion_decided(&self, doc_id, suggestion_id, accepted) { ... }
    pub fn title_updated(&self, doc_id, title) { ... }
    pub fn locks_changed(&self, doc_id) { ... }
}
```

### 前端适配

`web/src/App.tsx` 的 WebSocket 事件处理从 switch `type` 改为匹配具体事件类型，做增量更新而非全量刷新。

### 调用点变更

| 文件 | 变更 |
|------|------|
| `src/api.rs` | `hub.document_changed(...)` → `hub.lines_inserted(...)` 等 |
| `src/mcp.rs` | `hub.document_changed(...)` → `hub.lines_inserted(...)` 等 |
| `src/realtime.rs` | 删除 `document_changed`，新增各事件方法 |
| `web/src/App.tsx` | 事件处理匹配具体类型 |

---

## 实施顺序

1. db.rs 拆分（1）— 纯结构化，不改变行为，先行
2. 权限贯通（3）— 改变权限检查逻辑，依赖拆分后的模块结构
3. ActorKind 审计（4）— 改动最小，audit_tx 一处
4. DocumosaOps trait（2）— 新增 ops.rs，依赖拆分后的模块
5. WebSocket 事件细化（5）— 改动面最大，最后做

---

## 验证

- `cargo test` — 所有 54 个测试保持通过
- `cargo check --all-targets` — lib + tests + benchmarks 编译通过
- 如有新增测试，覆盖权限边界和事件序列化
