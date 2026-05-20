# Phase 2.1: AINRF 引入 documosa-core

> 目标读者：AINRF 开发者
> 状态：待实现

---

## 背景

AINRF（Agent-Iterated Numerical Research Framework）在 Live Research Manager 中管理 session trace、任务描述和实验报告。当前这些数据以非结构化文本存储。引入 `documosa-core` 可以将它们结构化为 Page/Block/RichText 文档，支持协作编辑和历史审计。

---

## 集成方式

AINRF 有两种使用方式，互不排斥：

### 方式 A：使用 documosa-core 模型（Cargo 依赖）

```toml
# ainrf/Cargo.toml
[dependencies]
documosa-core = { git = "https://github.com/Kozmosa/documosa.git", branch = "main" }
```

不依赖 SQLite/axum/tokio——只拿数据模型和 trait 签名。

```rust
use documosa_core::models::{Page, Block, RichTextToken, Annotations};
use documosa_core::identity::{Identity, RoleMode, ActorKind};
use documosa_core::operations::DocumosaOps;
```

**适用场景**：AINRF 用自己的存储后端（如 JSON 文件、Git 仓库），但复用 Documosa 的数据模型和权限语义，保证与 documosa server 交换时的格式一致。

### 方式 B：调用 documosa server API

AINRF 启动 documosa server 进程（或连接已有实例），通过 HTTP API 读写文档。

```bash
documosa serve --port 4317 --data-dir ~/.ainrf/documosa
```

然后通过 REST 调用：

| AINRF 操作 | Documosa API |
|-----------|-------------|
| 创建 session 报告 | `POST /v1/pages` |
| 追加 agent 思考记录 | `PATCH /v1/pages/{id}/children` |
| 读取完整报告 | `GET /v1/pages/{id}/snapshot` |
| 导出 Markdown | `GET /v1/pages/{id}/export/md` |
| 查看历史审计 | `GET /v1/pages/{id}/history` |

**Agent 身份**：AINRF 调用时在 MCP 或 HTTP 中使用 `ActorKind::Agent { agent_id, session_ref, task_ref }`，让审计日志记录是哪个模型、哪个 session 产生了变更。

---

## 具体映射

### Session → Page

```
Session {
    id: "ainrf-session-xxx",
    trace: Vec<AgentStep>,
}
```

每个 session 创建一个 Page：
```json
POST /v1/pages
{
    "properties": {
        "title": { "title": [{"text": {"content": "AINRF Session 2026-05-20"}}] }
    }
}
```

每个 agent step 作为 Block 追加到 Page 下：
```json
PATCH /v1/pages/{page_id}/children
{
    "children": [
        {
            "block_type": "heading_2",
            "content_json": "[{\"type\":\"text\",\"text\":{\"content\":\"Step 3: Code Generation\"}}]"
        },
        {
            "block_type": "code",
            "content_json": "[{\"type\":\"text\",\"text\":{\"content\":\"fn main() { ... }\"}}]",
            "properties_json": "{\"language\":\"rust\"}"
        }
    ]
}
```

### Experiment Report → Page

实验报告天然是文档结构（标题、段落、代码块、表格、公式）。AINRF 生成的结构化报告直接映射到 Notion block 类型。

### MCP Agent 交互

AINRF 作为 MCP client 连接 documosa 的 MCP endpoint：

```
POST /mcp  (JSON-RPC 2.0)

Tools:
  page_create(title)           -- 创建报告
  block_append(page_id, blocks) -- 追加内容
  block_update(block_id, ...)  -- 修改已有 block
  page_export(page_id)         -- 导出 Markdown
  history_list(page_id)        -- 查看变更历史
```

参数中包含 `actor_kind: "agent"` 和 `agent_id/session_ref/task_ref`，使所有变更可追溯到具体 agent 步骤。

---

## 工作量估计

| 组件 | 工作量 |
|------|--------|
| Cargo.toml 加依赖 | 1 行 |
| Session → Page 映射逻辑 | ~80 行 Rust |
| Agent step → Block 追加 | ~60 行 Rust |
| MCP 客户端集成 | ~100 行 Rust |
| 测试 | ~80 行 |

**总计：~320 行，轻量集成。**

---

## 非目标

- 不要求 AINRF 替换自己的存储后端
- 不要求 AINRF 实现 DocumosaOps trait
- 不做实时 WebSocket 同步（AINRF 用 REST 即可）
