# Phase 2.2: MMDash 引入 documosa-core

> 目标读者：MMDash 开发者
> 状态：待实现

---

## 背景

MMDash 当前通过 `documosa_provider.py` 调用 documosa 的 `/api/mmdash/*` 适配层（`mmdash_api.rs`）。这个适配层是为了兼容 mmdash 的旧 `doc_server` API 格式。现在 documosa v2.0 已经全面对齐 Notion API（`/v1/*`），mmdash 可以直接使用 Notion 兼容的 API，获得完整的 block 编辑能力。

---

## 迁移路径

### 选项 A：保持适配层，只换端点（零改动）

Documosa 的 `/api/mmdash/*` 仍正常工作。MMDash 无需任何改动即可继续使用。

当前 `DocumentProvider` 实现：
```python
# mmdash/backend/app/services/documosa_provider.py
class DocumosaProvider(DocumentProvider):
    async def fetch_page_content(self, page_id, credentials):
        url = f"{self.base_url}/api/mmdash/documents/{page_id}/content"
        ...
```

**优点**：零风险。**缺点**：用不到 v2.0 新功能（block 级操作、评论、suggestion、搜索等）。

### 选项 B：新增 `/v1/*` provider（推荐）

新增 `NotionDocumosaProvider`（或升级现有 `DocumosaProvider`），直接使用 Notion 兼容的 `/v1/*` API：

```python
class DocumosaV1Provider(DocumentProvider):
    provider_type = "documosa_v1"

    async def fetch_page_content(self, page_id, credentials):
        # 旧: /api/mmdash/documents/{id}/content
        # 新: /v1/pages/{id}/snapshot
        snap = await self._get(f"/v1/pages/{page_id}/snapshot", credentials)
        blocks = self._convert_blocks_to_mmdash(snap["blocks"])
        markdown = self._blocks_to_markdown(blocks)
        return {"page_id": page_id, "title": page_title(snap), "blocks": blocks, "markdown": markdown}

    async def update_page_content(self, page_id, content, credentials):
        # 旧: /api/mmdash/documents/{id}/content PUT
        # 新: DELETE all blocks + PATCH /v1/pages/{id}/children
        ...
```

### 选项 C：替换 doc_server 为 documosa（推荐长期）

MMDash 当前的 `doc_server/`（FastAPI 服务，存储页面为 JSON 文件）可以直接替换为 documosa。Documosa v2.0 已完全覆盖 `doc_server` 的功能：

| doc_server 端点 | documosa v2.0 等价端点 |
|----------------|----------------------|
| `GET /api/pages` | `GET /v1/pages` |
| `POST /api/pages` | `POST /v1/pages` |
| `GET /api/pages/{id}` | `GET /v1/pages/{id}/snapshot` |
| `PUT /api/pages/{id}` | `PATCH /v1/pages/{id}` + `PATCH /v1/pages/{id}/children` |
| `DELETE /api/pages/{id}` | soft-delete via `DELETE /v1/blocks/{id}`（级联） |

**额外获得**：
- Block 级历史审计（`GET /v1/pages/{id}/history`）
- 评论系统（`POST /v1/blocks/{id}/comments`）
- 全文搜索（`POST /v1/search`）
- 实时 WebSocket（`WS /v1/pages/{id}/ws`）
- 用户在线状态

---

## API 格式迁移

### 响应变化（旧 → 新）

**旧 `/api/mmdash/documents/{id}/content`**：
```json
{
    "page_id": "...",
    "title": "My Doc",
    "blocks": [{"type": "paragraph", "content": "Hello"}],
    "markdown": "Hello"
}
```

**新 `/v1/pages/{id}/snapshot`**：
```json
{
    "object": "page",
    "page": {
        "id": "...",
        "properties": {
            "title": {
                "title": [{"plain_text": "My Doc"}]
            }
        }
    },
    "blocks": [
        {
            "object": "block",
            "id": "...",
            "block_type": "paragraph",
            "content_json": "[{\"plain_text\":\"Hello\"}]"
        }
    ]
}
```

`title` 从裸字符串变为 `properties.title.title[]` rich_text 数组，需要解包。

---

## 认证

MMDash 已有 JWT 认证（`SECRET_KEY`）。Documosa 使用同一个 JWT 密钥。只需配置：

```bash
# documosa .env
JWT_SECRET=<same as mmdash SECRET_KEY>
```

MMDash 调用 documosa 时传 `Authorization: Bearer <token>`。

---

## 工作量估计

| 选项 | 工作量 | 说明 |
|------|--------|------|
| A: 不动 | 0 | 现有适配层继续工作 |
| B: 新增 v1 provider | ~200 行 Python | 新增/升级 Provider 类 |
| C: 替换 doc_server | ~300 行 Python + 删除 doc_server/ | 需迁移所有调用点 |

---

## 建议实施顺序

1. **先做 B**：新增 `DocumosaV1Provider`，与旧 provider 并存
2. **mmdash 配置切换**：团队可选择使用 `documosa_v1` provider
3. **验证稳定后做 C**：删除 `doc_server/`，默认 provider 改为 documosa

---

## 非目标

- 不要求 mmdash 用 Rust 依赖 documosa-core（Python 项目用 REST 即可）
- 不做 mmdash 前端 Tiptap 替换（mmdash 用 CodeMirror 已够用）
- 不替换 mmdash 的 Redis draft cache（autosave 保留）
