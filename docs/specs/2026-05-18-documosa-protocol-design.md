# Documosa v1.0: Document as a UI Protocol

> documosa-core 保持薄——只定义数据模型、操作签名、权限矩阵常量。不做状态机、不做 markdown 感知。

---

## 1. 定位

Documosa 是 **"Document as a UI for Human-Agent Collaboration"** 的协议定义 + 参考实现。

- **协议**（documosa-core）：文档作为人机之间所有信息交换的载体，定义了操作面、权限面、协商面
- **参考实现**（documosa）：REST + WebSocket + MCP server + 内置 SQLite + 后端适配器
- **前端**（documosa-web）：React 单页应用

---

## 2. 设计原则

| # | 原则 | 说明 |
|---|------|------|
| P1 | 规范格式是纯文本行序列 | 内部存储 = plain text lines。markdown 只是字符，不做结构感知 |
| P2 | 协议薄，实现厚 | core 只定义数据模型 + 操作签名 + 权限矩阵。状态机留给 server |
| P3 | 后端适配器只做格式转换 | 外部格式（Notion/飞书）↔ 纯文本字符串。行拆分、锁、审计在纯文本层统一 |
| P4 | Agent 是一等公民 | Identity 区分 Human/Agent 出处；MCP 是 Agent 原生入口 |
| P5 | 所有变更可追溯 | 每次操作为 AuditEvent；每行的作者/出处可追溯 |

---

## 3. Crate 结构

```
documosa-core/                  # 协议定义，零 I/O 依赖
├── models.rs                   # Document, Line, Identity, Comment, Suggestion, AuditEvent
├── identity.rs                 # Identity + ActorKind (Human | Agent)
├── operations.rs               # 操作签名 + 参数类型 + 返回值类型
├── permissions.rs              # 权限矩阵常量
└── error.rs                    # 协议级错误类型

documosa/                       # 参考实现
├── api.rs                      # REST + WebSocket
├── mcp.rs                      # MCP endpoint（Agent 入口）
├── cli.rs                      # CLI（human 入口）
├── db.rs                       # 内置 SQLite 存储
├── diff.rs                     # 文档/版本 diff
├── adapters/
│   ├── mod.rs                  # DocBackend trait
│   ├── local.rs                # SQLite（默认）
│   ├── notion.rs               # Notion ↔ 纯文本
│   └── feishu.rs               # 飞书 ↔ 纯文本（待实现）
└── realtime.rs                 # WebSocket 事件推送

documosa-web/                   # React 前端（已有）
```

---

## 4. 数据模型

### 4.1 文档 (Document)

```rust
// documosa-core/models.rs

struct Document {
    id: String,            // UUID
    title: String,
    created_at: String,    // RFC 3339
    updated_at: String,
}
```

### 4.2 行 (Line)

```rust
struct Line {
    id: String,            // UUID
    document_id: String,
    order_index: i64,      // 排序，支持间隙插入（step = 1000）
    content: String,       // 纯文本，可为空字符串
    revision: i64,         // 单调递增
    deleted: bool,
    created_at: String,
    updated_at: String,
}
```

### 4.3 身份与出处 (Identity)

```rust
// documosa-core/identity.rs

enum ActorKind {
    Human,
    Agent {
        agent_id: String,           // "claude-opus-4-7"
        session_ref: Option<String>, // AINRF session ID
        task_ref: Option<String>,    // AINRF task ID
    },
}

struct Identity {
    client_id: String,     // 持久化客户端标识
    nickname: String,      // 显示名
    role_mode: RoleMode,
    actor_kind: ActorKind,
}

enum RoleMode {
    Reviewer,  // 只读内容 + 评论 + 提 suggestion
    Writer,    // 写行 + 锁行 + 处理 suggestion
}
```

### 4.4 行锁 (LineLock)

```rust
struct LineLock {
    line_id: String,
    document_id: String,
    owner_client_id: String,
    owner_nickname: String,
    expires_at: String,
}
```

### 4.5 评论 (Comment)

```rust
struct Comment {
    id: String,
    document_id: String,
    start_line_id: String,
    end_line_id: String,
    start_column: Option<i64>,
    end_column: Option<i64>,
    author_client_id: String,
    author_nickname: String,
    role_mode: String,     // 评论时的角色
    body: String,          // 纯文本
    resolved: bool,
    created_at: String,
    updated_at: String,
}

struct CommentReply {
    id: String,
    comment_id: String,
    author_client_id: String,
    author_nickname: String,
    role_mode: String,
    body: String,          // 纯文本
    created_at: String,
}
```

### 4.6 建议 (Suggestion)

```rust
enum SuggestionKind {
    InsertLines,
    ReplaceLines,
    DeleteLines,
}

enum SuggestionState {
    Open,
    Accepted,
    Rejected,
}

struct Suggestion {
    id: String,
    document_id: String,
    kind: SuggestionKind,
    anchor_line_id: Option<String>,
    start_line_id: Option<String>,
    end_line_id: Option<String>,
    content_json: String,         // 建议的新内容
    base_revisions_json: String,  // 基于的行版本
    state: SuggestionState,
    author_client_id: String,
    author_nickname: String,
    role_mode: String,
    created_at: String,
    decided_by_client_id: Option<String>,
    decided_by_nickname: Option<String>,
    decided_at: Option<String>,
}
```

### 4.7 审计事件 (AuditEvent)

```rust
struct AuditEvent {
    id: String,
    document_id: String,
    actor_client_id: String,
    actor_nickname: String,
    role_mode: String,
    event_type: String,   // "line.inserted" | "line.replaced" | "comment.created" | ...
    details_json: String, // 操作详情
    created_at: String,
    note_body: Option<String>,
    note_updated_by_nickname: Option<String>,
    note_updated_at: Option<String>,
}
```

### 4.8 文档快照 (DocumentSnapshot)

```rust
/// 一次查询获取文档完整状态
struct DocumentSnapshot {
    document: Document,
    lines: Vec<Line>,
    comments: Vec<Comment>,
    replies: Vec<CommentReply>,
    suggestions: Vec<Suggestion>,
    locks: Vec<LineLock>,
    audit_events: Vec<AuditEvent>,
}
```

---

## 5. 操作签名

### 5.1 文档级操作

```rust
// documosa-core/operations.rs

// 创建文档（初始内容一次性写入，纯文本字符串）
fn create_document(actor: &Identity, title: String, content: String)
    -> Result<DocumentSnapshot>;

// 列出所有文档
fn list_documents() -> Result<Vec<Document>>;

// 获取文档完整状态
fn get_document(document_id: &str) -> Result<DocumentSnapshot>;

// 导出文档为纯文本字符串
fn export_document(document_id: &str) -> Result<String>;

// 更新标题
fn update_title(actor: &Identity, document_id: &str, title: &str) -> Result<()>;
```

### 5.2 行操作

```rust
// 插入行（在 anchor_line_id 之后，若为 None 则在末尾）
fn insert_lines(actor: &Identity, document_id: &str,
    anchor_line_id: Option<&str>, lines: Vec<String>) -> Result<Vec<Line>>;

// 替换行内容
fn replace_lines(actor: &Identity, document_id: &str,
    line_ids: Vec<&str>, new_contents: Vec<String>,
    base_revisions: Vec<BaseRevision>) -> Result<Vec<Line>>;

// 删除行（软删除）
fn delete_lines(actor: &Identity, document_id: &str,
    line_ids: Vec<&str>) -> Result<Vec<Line>>;
```

### 5.3 行锁操作

```rust
// 锁定行（Writer 独占）
fn lock_lines(actor: &Identity, document_id: &str,
    line_ids: Vec<&str>, ttl_seconds: i64) -> Result<Vec<LineLock>>;

// 心跳续期
fn heartbeat_locks(actor: &Identity, document_id: &str,
    line_ids: Vec<&str>, ttl_seconds: i64) -> Result<Vec<LineLock>>;

// 释放锁
fn release_locks(actor: &Identity, document_id: &str,
    line_ids: Vec<&str>) -> Result<()>;
```

### 5.4 评论操作

```rust
fn create_comment(actor: &Identity, document_id: &str,
    start_line_id: &str, end_line_id: &str,
    start_column: Option<i64>, end_column: Option<i64>,
    body: &str) -> Result<Comment>;

fn reply_comment(actor: &Identity, document_id: &str,
    comment_id: &str, body: &str) -> Result<CommentReply>;

fn update_comment(actor: &Identity, document_id: &str,
    comment_id: &str, body: &str) -> Result<Comment>;

fn resolve_comment(actor: &Identity, document_id: &str,
    comment_id: &str) -> Result<Comment>;
```

### 5.5 Suggestion 操作

```rust
fn create_suggestion(actor: &Identity, document_id: &str,
    kind: SuggestionKind, anchor_line_id: Option<&str>,
    start_line_id: Option<&str>, end_line_id: Option<&str>,
    new_content: &str, base_revisions: Vec<BaseRevision>) -> Result<Suggestion>;

fn accept_suggestion(actor: &Identity, document_id: &str,
    suggestion_id: &str) -> Result<Suggestion>;

fn reject_suggestion(actor: &Identity, document_id: &str,
    suggestion_id: &str) -> Result<Suggestion>;
```

### 5.6 历史与 Diff 操作

```rust
fn list_history(document_id: &str, category: HistoryCategory,
    from: Option<&str>, to: Option<&str>, limit: i64) -> Result<Vec<AuditEvent>>;

fn history_diff(document_id: &str,
    from_event_id: &str, to_event_id: &str) -> Result<HistoryDiff>;

fn set_audit_note(actor: &Identity, document_id: &str,
    audit_event_id: &str, body: &str) -> Result<()>;
```

---

## 6. 权限矩阵

```rust
// documosa-core/permissions.rs

/// 每个 RoleMode 允许的操作集合。
/// 声明性常量，不实现状态机——具体 enforcement 由 server 层负责。
const WRITER_OPS: &[&str] = &[
    "create_document",
    "insert_lines", "replace_lines", "delete_lines",
    "lock_lines", "heartbeat_locks", "release_locks",
    "create_comment", "reply_comment", "update_comment", "resolve_comment",
    "create_suggestion",   // Writer 也可以提建议（对他人内容）
    "accept_suggestion", "reject_suggestion",
    "view_history", "history_diff", "set_audit_note",
    "export_document",
];

const REVIEWER_OPS: &[&str] = &[
    "create_document",
    "create_comment", "reply_comment", "update_comment", "resolve_comment",
    "create_suggestion",   // Reviewer 通过 suggestion 表达修改意图
    "view_history", "history_diff", "set_audit_note",
    "export_document",
];

fn is_allowed(role: RoleMode, operation: &str) -> bool {
    match role {
        Writer => WRITER_OPS.contains(&operation),
        Reviewer => REVIEWER_OPS.contains(&operation),
    }
}
```

**关键设计**：
- Reviewer 不能直接改内容行，但可以通过 `create_suggestion` 表达修改意图
- Writer 可以创建 suggestion（针对他人写的内容），但主要是 accept/reject
- 双方都可以评论、回复、标记解决

---

## 7. Suggestion 工作流：人机协商的核心机制

```
Human (Reviewer)              Document               Agent (Writer)
────────────────────────────────────────────────────────────
Agent 先写了一版草案               │
                                  │  [Agent 写行]
Human 看到不认可的行              │
                                  │
Human 创建 Suggestion ───────────→│
  kind: replace_lines            │
  new_content: "<修正后的文本>"    │
                                  │  Agent 收到 suggestion
                                  │
                    ←─────────────│  Agent 接受
                                  │  → 内容被替换
                                  │  → audit 记录 "suggestion.accepted"
                                  │
或者：                            │
                    ←─────────────│  Agent 拒绝
                                  │  → 内容不变
                                  │  → audit 记录 "suggestion.rejected"
                                  │  → Agent 通常在 comment 里解释原因
```

**Suggestion 不只是一个 UI 功能——它是人机协商的正式协议**：
- Human 用 Suggestion 表达 "我觉得应该这样改"
- Agent 用 accept/reject + comment 表达 "同意/不同意，理由如下"
- 全部可追溯（AuditEvent 记录完整 negotiation 链）

---

## 8. 后端适配器 Trait

```rust
// documosa/adapters/mod.rs

/// 外部文档系统 ↔ 纯文本字符串的双向转换。
/// 每个适配器的唯一职责。不涉及行拆分、锁、审计。
trait DocBackend {
    /// 外部系统 → 纯文本 content string
    async fn import(&self, external_id: &str) -> Result<String>;

    /// 纯文本 content string → 外部系统
    async fn export(&self, external_id: &str, content: &str) -> Result<()>;

    /// 列出外部系统中的文档引用
    async fn list(&self, parent_ref: &str) -> Result<Vec<DocRef>>;
}

struct DocRef {
    external_id: String,
    title: String,
    updated_at: String,
}
```

内置实现计划：

| 适配器 | import | export | 状态 |
|--------|--------|--------|------|
| Local (SQLite) | —（原生就是纯文本） | — | 已有 |
| Notion | Notion blocks → text | text → Notion paragraphs | 已有 |
| 飞书 | 飞书 doc → text | text → 飞书 doc | 待实现 |
| MCP (外部 Agent) | 通过 MCP 读取外部源 | 通过 MCP 写入外部源 | 待设计 |

---

## 9. 集成面

### 9.1 Agent 入口：MCP

```
POST /mcp  (JSON-RPC 2.0, Streamable HTTP)

tools:
  document_create(title, content)           → document_id
  document_get(document_id)                 → DocumentSnapshot
  document_export(document_id)              → plain text string
  line_insert(document_id, anchor, lines[]) → lines
  line_replace(document_id, line_ids[], contents[], revisions[]) → lines
  line_delete(document_id, line_ids[])      → lines
  comment_create(document_id, start_line, end_line, body)
  suggestion_create(document_id, kind, lines[], new_content)
  suggestion_accept(document_id, suggestion_id)
  suggestion_reject(document_id, suggestion_id)
  history_list(document_id, category, limit)
  history_diff(document_id, from_event, to_event)
```

### 9.2 人类入口：REST + WebSocket

```
GET  /api/documents
POST /api/documents
GET  /api/documents/{id}
PUT  /api/documents/{id}/content
GET  /api/documents/{id}/export

POST /api/documents/{id}/lines/insert
POST /api/documents/{id}/lines/replace
POST /api/documents/{id}/lines/delete

POST /api/documents/{id}/locks/heartbeat
POST /api/documents/{id}/locks/release

POST /api/documents/{id}/comments
POST /api/documents/{id}/comments/{id}/reply
POST /api/documents/{id}/comments/{id}/resolve

POST /api/documents/{id}/suggestions
POST /api/documents/{id}/suggestions/{id}/accept
POST /api/documents/{id}/suggestions/{id}/reject

GET  /api/documents/{id}/history
GET  /api/documents/{id}/history-diff

WS   /ws  (realtime events: line.changed, comment.created, suggestion.updated, ...)
```

### 9.3 被 AINRF 嵌入

AINRF 在实现 Live Research Manager 时，可以直接依赖 `documosa-core`：

```rust
// ainrf 使用 documosa-core 的模型
use documosa_core::models::{Document, DocumentSnapshot, Identity, ActorKind, RoleMode};
use documosa_core::operations;  // 操作签名，用于类型约束

// AINRF 的 session trace → Documosa document
// AINRF 的任务描述 → Documosa document
// AINRF 的实验报告 → Documosa document
```

不需要启动 documosa server——AINRF 可以自己实现存储后端（用 documosa-core 的模型），也可以调用 documosa server 的 API。

### 9.4 被 MMDash 嵌入

```rust
// MMDash 使用 documosa-core 的模型显示文档状态
// MMDash 的模型文档编辑 → Documosa document
// MMDash 的 team 协作 → Documosa 权限模型
```

---

## 10. 非目标

- 不做 markdown 解析或结构感知（heading/code block 等）——Agent 自己 parse
- 不做 WYSIWYG 富文本编辑器——前端可用 CherryEditor/Milkdown 等，但存储始终是纯文本
- 不做实时协同编辑（多人同时写同一行）——通过行锁串行化
- 不做全文搜索——Agent 自己 grep
- 不做文档间链接解析——Agent 自己处理 markdown wikilink
- 不做 AI 自动补全/续写——那是 Agent 的事，不是协议的事

---

## 11. 路线图

### Phase 1: crate 拆分 + protocol 定义（当前）

1. 从 `documosa/` 抽取 `documosa-core/`（models + identity + operations + permissions + error）
2. `documosa/` 改为依赖 `documosa-core`
3. `identity.rs` 增加 `ActorKind`（Human | Agent）
4. `permissions.rs` 提取权限矩阵
5. MCP tools 保持不变，底层切换到 core 模型

### Phase 2: 集成面打磨

1. AINRF 引入 `documosa-core` 作为依赖
2. MMDash 引入 `documosa-core` 作为依赖
3. 飞书适配器

### Phase 3: ARCLI 联动

1. ARCLI 的实验报告、claim 文档可以通过 Documosa 协作编辑
2. `arcli doc open <exp_id>` → 在 Documosa 中打开该实验的结构化报告
