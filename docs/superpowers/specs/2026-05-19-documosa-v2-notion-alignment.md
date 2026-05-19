# Documosa v2.0: Tiptap Editor + Notion Block Storage — 设计文档

> 2026-05-19 | documosa 全面对齐 Notion：Tiptap 编辑器 + Block 存储 + Notion API

---

## 1. 定位

Documosa v2.0 放弃 v1.0 的纯文本行存储 + Markdown 编辑器路线，全面对齐 Notion 的 block 存储模型和编辑器体验，同时保留 documosa 特有的 Agent 协商能力（Comment / Suggestion / Audit / MCP）。

---

## 2. Block 数据模型

### 2.1 Document → Page

```rust
struct Page {
    id: String,
    title: String,
    properties_json: String,   // "{}"
    created_at: String,
    updated_at: String,
}
```

### 2.2 Block

```rust
struct Block {
    id: String,                    // UUID
    page_id: String,
    parent_id: Option<String>,     // null = root block
    order_index: f64,              // 浮点排序，支持任意位置插入
    block_type: String,            // paragraph | heading_1 | code | equation | ...
    content_json: String,          // RichTextToken[] JSON
    properties_json: String,       // { "language": "python" } | { "checked": true }
    revision: i64,
    deleted: bool,
    created_at: String,
    updated_at: String,
}
```

### 2.3 Rich Text

```rust
type RichText = Vec<RichTextToken>;

struct RichTextToken {
    r#type: String,               // "text" | "mention" | "equation"
    text: Option<TextContent>,    // type=text 时 { content, link }
    annotations: Annotations,
    plain_text: String,
    href: Option<String>,
}

struct TextContent {
    content: String,
    link: Option<String>,
}

struct Annotations {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    underline: bool,
    code: bool,
    color: String,                // "default" | "gray" | "brown" | "orange" | ...
}
```

### 2.4 支持的 Block 类型

| Block Type | properties 字段 | 说明 |
|---|------|------|
| `paragraph` | — | 普通段落 |
| `heading_1` / `heading_2` / `heading_3` | — | 标题 |
| `bulleted_list_item` | — | 无序列表 |
| `numbered_list_item` | — | 有序列表 |
| `to_do` | `checked` | 待办 |
| `toggle` | — | 折叠块 |
| `code` | `language` | 代码块 |
| `equation` | — | 公式块（mmdash 特有，对标 Notion paragraph + inline equation） |
| `quote` | — | 引用 |
| `callout` | `icon`, `color` | 标注 |
| `divider` | — | 分隔线 |
| `image` | `url`, `caption` | 图片 |
| `table` | `table_width`, `has_column_header`, `has_row_header` | 表格容器 |
| `table_row` | — | 表格行（cells 在 content_json 中） |
| `column_list` | — | 分栏容器 |
| `column` | — | 分栏 |
| `child_page` | `page_id` | 子页面引用 |

### 2.5 现有模型迁移

| v1.0 模型 | v2.0 处理 |
|-----------|----------|
| **Document** | 重命名为 **Page** |
| **Line** | 替换为 **Block** |
| **LineLock** | 重命名为 **BlockLock**，`line_id` → `block_id` |
| **Comment** | `start_line_id/end_line_id` → `target_block_id` |
| **CommentReply** | 不变 |
| **Suggestion** | `anchor_line_id/start_line_id/end_line_id` → `target_block_id` |
| **AuditEvent** | 不变（`details_json` 中的 line_id 引用改为 block_id） |
| **DocumentSnapshot** | 重命名为 **PageSnapshot** |
| **HistoryDiff** | 不变 |
| **BaseRevision** | 不变 |
| **HistoryCategory** | 不变 |

---

## 3. REST API 设计（对标 Notion API）

### 3.1 页面操作

```
POST   /v1/pages                        → 创建页面
GET    /v1/pages/{page_id}              → 获取页面元数据
PATCH  /v1/pages/{page_id}              → 更新 title / properties
GET    /v1/pages/{page_id}/snapshot     → 完整快照（Page + Blocks + Comments + Suggestions + Locks + Audit）
```

### 3.2 Block 操作

```
GET    /v1/blocks/{block_id}            → 单个 Block
GET    /v1/blocks/{block_id}/children   → 子 Block 列表（?page_size=&start_cursor=）
PATCH  /v1/blocks/{block_id}            → 更新 block_type / content_json / properties_json
DELETE /v1/blocks/{block_id}            → 软删除 Block（级联子孙）
PATCH  /v1/pages/{page_id}/children     → 批量 append/insert children
                                          Body: { children: [Block], after: block_id? }
```

### 3.3 评论（锚定到 Block）

```
POST   /v1/blocks/{block_id}/comments                      → 创建评论
GET    /v1/blocks/{block_id}/comments                      → 列出评论
POST   /v1/blocks/{block_id}/comments/{comment_id}/replies   → 回复
PATCH  /v1/blocks/{block_id}/comments/{comment_id}         → 更新评论正文
DELETE /v1/blocks/{block_id}/comments/{comment_id}         → 删除评论
POST   /v1/blocks/{block_id}/comments/{comment_id}/resolve   → 标记解决
```

### 3.4 Suggestion

```
POST   /v1/pages/{page_id}/suggestions                     → 创建建议
POST   /v1/pages/{page_id}/suggestions/{suggestion_id}/accept → 接受
POST   /v1/pages/{page_id}/suggestions/{suggestion_id}/reject → 拒绝
```

### 3.5 历史 & 导出

```
GET    /v1/pages/{page_id}/history       → AuditEvent 列表（?category=&from=&to=&limit=）
GET    /v1/pages/{page_id}/history-diff  → 两个版本 diff（?from=&to=）
POST   /v1/pages/{page_id}/audit/{event_id}/note  → 设置审计笔记
GET    /v1/pages/{page_id}/export/md     → 导出 Markdown
```

### 3.6 WebSocket（Documosa 特有）

```
WS     /v1/pages/{page_id}/ws            → 实时事件流
  事件: presence, block_inserted, block_updated, block_deleted,
        comment_created, comment_resolved, suggestion_created,
        suggestion_decided, page_title_updated, locks_changed
```

### 3.7 MCP（Agent 入口）

```
POST   /mcp   (JSON-RPC 2.0)

tools:
  page_create(title)                             → page_id
  page_get(page_id)                              → PageSnapshot
  block_get(block_id)                            → Block
  block_list_children(page_id, parent_id?, cursor?, page_size?) → Vec<Block>
  block_append(page_id, blocks[], after?)        → Vec<Block>
  block_update(block_id, content, block_type, properties) → Block
  block_delete(block_id)                         → Block
  comment_create(block_id, body)                 → Comment
  reply_comment(block_id, comment_id, body)      → CommentReply
  resolve_comment(block_id, comment_id)          → Comment
  suggestion_create(page_id, kind, target_block_id, new_blocks[]) → Suggestion
  suggestion_accept(page_id, suggestion_id)      → Suggestion
  suggestion_reject(page_id, suggestion_id)      → Suggestion
  history_list(page_id, category, limit)         → Vec<AuditEvent>
  history_diff(page_id, from_event, to_event)    → HistoryDiff

参数: client_id, nickname, role_mode, actor_kind (agent 时传 agent_id/session_ref/task_ref)
```

**MCP 双格式支持**：
- 每个创建/更新 block 的 tool 接受 `content_format` 参数
- `content_format: "rich_text"`（默认）— 传 Notion style rich_text JSON
- `content_format: "markdown"` — 传 Markdown 字符串，服务端用 `markdown_to_blocks` → `blocks_to_rich_text` 转换

### 3.8 MMDash 适配层

```
GET    /api/mmdash/documents/{page_id}/content   → { page_id, title, blocks, markdown }
GET    /api/mmdash/documents/{page_id}           → { page_id, title, created_at, updated_at }
POST   /api/mmdash/documents                     → 创建页面
PUT    /api/mmdash/documents/{page_id}/content   → 更新内容
```

### 3.9 认证

```
Authorization: Bearer <token>  — JWT（替代旧的 x-documosa-* LAN header）
```

### 3.10 删除的旧 API

全部 `/api/documents/*` 端点删除，包括：
- `GET  /api/documents`
- `POST /api/documents`
- `GET  /api/documents/{id}`
- `PUT  /api/documents/{id}/content`
- `POST /api/documents/{id}/lines/insert`
- `POST /api/documents/{id}/lines/replace`
- `POST /api/documents/{id}/lines/delete`
- `POST /api/documents/{id}/locks/heartbeat`
- `POST /api/documents/{id}/locks/release`
- `POST /api/documents/{id}/comments`
- `POST /api/documents/{id}/suggestions`
- `GET  /api/documents/{id}/history`
- `GET  /api/documents/{id}/history-diff`
- `POST /api/documents/{id}/audit-events/{id}/note`
- `GET  /api/documents/{id}/export`
- `WS   /api/documents/{id}/ws`

---

## 4. 存储层 —— SQLite

### 4.1 表结构

```sql
CREATE TABLE pages (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    properties_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE blocks (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES blocks(id) ON DELETE CASCADE,
    order_index REAL NOT NULL,
    block_type TEXT NOT NULL,
    content_json TEXT NOT NULL DEFAULT '[]',
    properties_json TEXT NOT NULL DEFAULT '{}',
    revision INTEGER NOT NULL DEFAULT 1,
    deleted INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_blocks_page_order ON blocks(page_id, parent_id, deleted, order_index);
CREATE INDEX idx_blocks_parent ON blocks(parent_id, deleted, order_index);

CREATE TABLE block_locks (
    block_id TEXT PRIMARY KEY REFERENCES blocks(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    owner_client_id TEXT NOT NULL,
    owner_nickname TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE comments (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    target_block_id TEXT NOT NULL,
    start_column INTEGER,
    end_column INTEGER,
    author_client_id TEXT NOT NULL,
    author_nickname TEXT NOT NULL,
    role_mode TEXT NOT NULL,
    body TEXT NOT NULL,
    resolved INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE comment_replies (
    id TEXT PRIMARY KEY,
    comment_id TEXT NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    author_client_id TEXT NOT NULL,
    author_nickname TEXT NOT NULL,
    role_mode TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE suggestions (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    target_block_id TEXT,
    content_json TEXT NOT NULL,
    base_revisions_json TEXT NOT NULL,
    state TEXT NOT NULL,
    author_client_id TEXT NOT NULL,
    author_nickname TEXT NOT NULL,
    role_mode TEXT NOT NULL,
    created_at TEXT NOT NULL,
    decided_by_client_id TEXT,
    decided_by_nickname TEXT,
    decided_at TEXT
);

CREATE TABLE audit_events (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    actor_client_id TEXT NOT NULL,
    actor_nickname TEXT NOT NULL,
    role_mode TEXT NOT NULL,
    event_type TEXT NOT NULL,
    details_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE audit_event_notes (
    audit_event_id TEXT PRIMARY KEY REFERENCES audit_events(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    updated_by_client_id TEXT NOT NULL,
    updated_by_nickname TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE page_versions (
    audit_event_id TEXT PRIMARY KEY REFERENCES audit_events(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);
```

### 4.2 关键设计

- **`order_index REAL`**：插入两个 block 之间取 `(prev + next) / 2.0`。只在浮点精度不够时重排
- **`parent_id`**：NULL = root block。删除父 block 时级联删除子孙
- **`content_json`**：完整 `RichTextToken[]`
- **`properties_json`**：block 类型的额外属性
- **软删除**：`blocks.deleted`，子孙不会自动标记删除，需要级联处理
- **分页**：cursor 用 `order_index`

---

## 5. 前端编辑器 —— Tiptap

### 5.1 技术栈

```
@tiptap/react
@tiptap/starter-kit
@tiptap/extension-code-block-lowlight
@tiptap/extension-table
@tiptap/extension-task-list / @tiptap/extension-task-item
@tiptap/extension-placeholder
lowlight           — 语法高亮
katex              — LaTeX 渲染
```

### 5.2 数据流

```
Tiptap Editor (ProseMirror JSON)
    │
    ├── 编辑时: ProseMirror JSON → documosa Block + RichText
    │     PATCH /v1/blocks/{id}/children  或  PATCH /v1/blocks/{id}
    │
    └── 读取时: documosa Blocks → ProseMirror JSON
         GET /v1/blocks/{page_id}/children（递归获取）
```

### 5.3 功能清单

- Notion 风格 UI：6-dot 拖拽手柄 + "+" 添加按钮
- Slash 命令面板（/h1 /h2 /h3 /code /eq /todo /quote /divider /table /image）
- 拖拽排序、缩进升降级
- 代码块 + 语法高亮（lowlight）
- 数学公式（KaTeX 渲染，mmdash 需要）
- 评论：选中文本 → 锚定到 block → 右侧评论面板
- 实时协作：WebSocket `block_updated` 增量更新
- 自动保存：2s 防抖 → 保存到服务端
- 导出 Markdown：`GET /v1/pages/{id}/export/md`

### 5.4 ProseMirror → Rich Text 映射

| ProseMirror Mark | Rich Text Annotation |
|------------------|---------------------|
| `bold` | `annotations.bold = true` |
| `italic` | `annotations.italic = true` |
| `strike` | `annotations.strikethrough = true` |
| `underline` | `annotations.underline = true` |
| `code` | `annotations.code = true` |
| `link { href }` | `text.link.url = href` |

### 5.5 ProseMirror Node → Block Type 映射

| ProseMirror Node | Block Type |
|-----------------|------------|
| `doc` | （page root） |
| `paragraph` | `paragraph` |
| `heading { level: 1/2/3 }` | `heading_1` / `heading_2` / `heading_3` |
| `bulletList` / `listItem` | `bulleted_list_item` |
| `orderedList` / `listItem` | `numbered_list_item` |
| `taskList` / `taskItem` | `to_do` |
| `codeBlock` | `code`（language 存 properties）|
| `blockquote` | `quote` |
| `horizontalRule` | `divider` |
| `table` | `table` / `table_row` |
| 自定义 `equation` | `equation` |
| 自定义 `image` | `image` |
| 自定义 `callout` | `callout` |
| 自定义 `toggle` | `toggle`（children 存到嵌套 blocks）|
| 自定义 `columnList` / `column` | `column_list` / `column` |

---

## 6. 模块结构

### 6.1 documosa-core（协议层）

```
documosa-core/src/
  models.rs       — Page, Block, RichTextToken, BlockLock, Comment, CommentReply,
                    Suggestion, SuggestionKind, AuditEvent, PageSnapshot, HistoryDiff,
                    BaseRevision, HistoryCategory
  identity.rs     — Identity, RoleMode, ActorKind（不变）
  operations.rs   — DocumosaOps trait（重写为 block 操作）
  permissions.rs  — WRITER_OPS, REVIEWER_OPS, is_allowed（更新操作名）
  error.rs        — ProtocolError（不变）
  lib.rs
```

### 6.2 documosa（参考实现）

```
src/
  main.rs
  lib.rs            — AppState (pool, hub, web_dir)

  api/
    mod.rs          — Router 组装
    pages.rs        — Page CRUD
    blocks.rs       — Block CRUD + children
    comments.rs     — Comment CRUD
    suggestions.rs  — Suggestion CRUD
    history.rs      — 历史 + diff
    export.rs       — Markdown 导出
    ws.rs           — WebSocket

  mcp.rs            — MCP tools（block 操作 + 双格式）

  db/
    mod.rs          — connect, connect_memory, migrate
    page.rs         — create_page, get_page, update_page_title, snapshot
    block.rs        — insert_block, update_block, delete_block, get_children, reorder
    lock.rs         — heartbeat_locks, release_locks
    comment.rs      — CRUD
    suggestion.rs   — create, decide
    history.rs      — list, note, diff
    audit.rs        — audit_tx, begin_write_tx, touch_page_tx
    ops.rs          — DocumosaOps impl
    permission.rs   — require_permission

  adapters/
    mod.rs          — DocBackend trait
    local.rs        — SQLite 默认
    notion.rs       — Notion ↔ Block
    feishu.rs       — 飞书（待实现）

  mmdash_api.rs     — MMDash 适配层（更新为 block 格式）
  mmdash_auth.rs    — JWT 验证
  mmdash_blocks.rs  — Markdown ↔ blocks 双向转换（保留）

  realtime.rs       — EventHub + WebSocket
  diff.rs           — diff 工具
  static_files.rs   — 静态文件服务
  error.rs          — AppError
  cli.rs            — CLI 入口
```

### 6.3 documosa-web（前端）

```
web/src/
  App.tsx            — 主应用
  TiptapEditor.tsx   — Tiptap 编辑器组件
  CommentPanel.tsx   — 评论面板
  HistoryPanel.tsx   — 历史面板
  lib/
    api.ts           — API 客户端
    converter.ts     — ProseMirror ↔ RichText 转换
    ws.ts            — WebSocket 客户端
  components/ui/     — shadcn UI 组件（保留）
```

---

## 7. 实施顺序

### Phase 1: 数据模型 + 存储迁移

1. 重写 `documosa-core/src/models.rs` — Line → Block, Document → Page
2. 重写 `documosa-core/src/operations.rs` — 所有签名改为 block 操作
3. 更新 `documosa-core/src/permissions.rs` — 操作名更新
4. 重写 SQLite migration — 新表结构
5. 重写 `db/` 模块 — block CRUD 替换 line CRUD
6. 更新 `DocumosaOps` impl

### Phase 2: API 层重写

7. 重写所有 REST handlers — `/v1/*` 替换 `/api/*`
8. 更新 WebSocket 事件 — block 事件替换 line 事件
9. 更新 MCP tools — block 操作 + 双格式支持
10. 更新 MMDash 适配层

### Phase 3: 前端 Tiptap 集成

11. 安装 Tiptap 依赖
12. 实现 ProseMirror ↔ RichText 转换
13. 构建编辑器组件
14. 实现评论面板
15. WebSocket 实时同步
16. 自动保存

### Phase 4: 测试 + 清理

17. 重写测试
18. 构建验证

---

## 8. 非目标

- 不做后端 block 合法性校验——Notion 也不做，Tiptap 前端保证结构有效
- 不做全文搜索——Agent 自己 grep
- 不做 block 间 link 自动解析——Agent 自己处理
- 不做 AI 自动补全——Agent 的事
- 不做 WYSIWYG 富文本工具栏——保留 slash 命令 + Markdown 快捷键
