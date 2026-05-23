# Simple Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a lightweight `/simple/editor` page for uploading, WYSIWYG-editing, auto-saving, and downloading one Markdown draft.

**Architecture:** The editor page is pure frontend for persistence: it stores one draft in `localStorage` and never creates Documosa pages. Markdown conversion is server-side and stateless through `/simple/editor/md2blocks` and `/simple/editor/blocks2md`, reusing `mmdash_blocks` with a small adapter to the existing frontend `DocumosaBlock` shape. The frontend entry point selects `SimpleEditorPage` for `/simple/editor` and keeps the main app unchanged for all other routes.

**Tech Stack:** Rust, Axum, serde, existing `mmdash_blocks`, React 19, TypeScript, Vite, existing `TiptapEditor`, localStorage, browser File/Blob APIs.

---

## File Structure

- Create `src/simple_editor.rs`
  - Owns stateless conversion routes under `/simple/editor`.
  - Defines request/response DTOs and adapter helpers between `mmdash_blocks::Block` and frontend-compatible `SimpleEditorBlock`.
  - Does not depend on `AppState`, the database, JWT auth, or collaboration state.

- Modify `src/lib.rs`
  - Export `simple_editor` module.
  - Merge `simple_editor::router()` before the static file fallback.

- Modify `tests/integration.rs`
  - Add API tests for Markdown to blocks, blocks to Markdown, and malformed JSON.

- Create `web/src/lib/simpleEditorApi.ts`
  - Thin typed client for `/simple/editor/md2blocks` and `/simple/editor/blocks2md`.

- Modify `web/src/TiptapEditor.tsx`
  - Allow external `blocks=[]` updates to clear the editor, needed by the simple editor clear action.

- Create `web/src/SimpleEditorPage.tsx`
  - Owns simple editor UI, single local draft, upload/download/clear, conversion API calls, and debounced localStorage writes.

- Modify `web/src/main.tsx`
  - Render `SimpleEditorPage` only when `window.location.pathname === '/simple/editor'`; render existing `App` otherwise.

---

### Task 1: Backend conversion API

**Files:**
- Create: `src/simple_editor.rs`
- Modify: `src/lib.rs`
- Test: `tests/integration.rs`

- [ ] **Step 1: Write failing API tests**

Add these tests near the other HTTP API integration tests in `tests/integration.rs`:

```rust
#[tokio::test]
async fn simple_editor_md2blocks_converts_markdown_without_auth() {
    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/simple/editor/md2blocks")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"markdown":"# Title\n\n- Item"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let blocks = body["blocks"].as_array().unwrap();
    assert_eq!(blocks[0]["block_type"], "heading_1");
    assert_eq!(blocks[0]["content_json"][0]["plain_text"], "Title");
    assert_eq!(blocks[1]["block_type"], "bulleted_list_item");
    assert_eq!(blocks[1]["content_json"].as_str(), None);
    assert_eq!(blocks[1]["content_json"][0]["plain_text"], "Item");
}

#[tokio::test]
async fn simple_editor_blocks2md_converts_blocks_without_auth() {
    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/simple/editor/blocks2md")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({
                    "blocks": [
                        {
                            "block_type": "heading_1",
                            "content_json": [{"type":"text","text":{"content":"Title"},"plain_text":"Title"}],
                            "properties_json": "{}"
                        },
                        {
                            "block_type": "paragraph",
                            "content_json": [{"type":"text","text":{"content":"Body"},"plain_text":"Body"}],
                            "properties_json": "{}"
                        }
                    ]
                }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["markdown"], "# Title\nBody");
}

#[tokio::test]
async fn simple_editor_md2blocks_rejects_malformed_json() {
    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/simple/editor/md2blocks")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test simple_editor_
```

Expected: the three tests fail with `404 Not Found` for `/simple/editor/*` routes.

- [ ] **Step 3: Create backend conversion module**

Create `src/simple_editor.rs`:

```rust
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::mmdash_blocks::{Block, BlockType, blocks_to_markdown, markdown_to_blocks};

pub fn router() -> Router {
    Router::new()
        .route("/simple/editor/md2blocks", post(md2blocks))
        .route("/simple/editor/blocks2md", post(blocks2md))
}

#[derive(Deserialize)]
struct Md2BlocksRequest {
    markdown: String,
}

#[derive(Serialize)]
struct Md2BlocksResponse {
    blocks: Vec<SimpleEditorBlock>,
}

#[derive(Deserialize)]
struct Blocks2MdRequest {
    blocks: Vec<SimpleEditorBlock>,
}

#[derive(Serialize)]
struct Blocks2MdResponse {
    markdown: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SimpleEditorBlock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    block_type: String,
    content_json: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    properties_json: Option<String>,
}

async fn md2blocks(Json(body): Json<Md2BlocksRequest>) -> Json<Md2BlocksResponse> {
    let blocks = markdown_to_blocks(&body.markdown)
        .into_iter()
        .map(SimpleEditorBlock::from_mmdash)
        .collect();
    Json(Md2BlocksResponse { blocks })
}

async fn blocks2md(Json(body): Json<Blocks2MdRequest>) -> Json<Blocks2MdResponse> {
    let blocks: Vec<Block> = body
        .blocks
        .iter()
        .map(SimpleEditorBlock::to_mmdash)
        .collect();
    Json(Blocks2MdResponse {
        markdown: blocks_to_markdown(&blocks),
    })
}

impl SimpleEditorBlock {
    fn from_mmdash(block: Block) -> Self {
        let block_type = match block.block_type {
            BlockType::Heading1 => "heading_1",
            BlockType::Heading2 => "heading_2",
            BlockType::Heading3 => "heading_3",
            BlockType::Code => "code",
            BlockType::Equation => "equation",
            BlockType::BulletedListItem => "bulleted_list_item",
            BlockType::NumberedListItem => "numbered_list_item",
            BlockType::Quote => "quote",
            BlockType::Divider => "divider",
            BlockType::Paragraph => "paragraph",
        }
        .to_string();

        let content = block.content.unwrap_or_default();
        let content_json = if block_type == "divider" {
            json!([])
        } else {
            json!([{
                "type": "text",
                "text": { "content": content },
                "plain_text": content
            }])
        };
        let properties_json = block
            .language
            .filter(|language| !language.is_empty())
            .map(|language| json!({ "language": language }).to_string());

        Self {
            id: None,
            block_type,
            content_json,
            properties_json,
        }
    }

    fn to_mmdash(&self) -> Block {
        let content = plain_text_from_content_json(&self.content_json);
        let language = self
            .properties_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|value| value.get("language").and_then(Value::as_str).map(str::to_string));

        let block_type = match self.block_type.as_str() {
            "heading_1" => BlockType::Heading1,
            "heading_2" => BlockType::Heading2,
            "heading_3" => BlockType::Heading3,
            "code" => BlockType::Code,
            "equation" => BlockType::Equation,
            "bulleted_list_item" => BlockType::BulletedListItem,
            "numbered_list_item" => BlockType::NumberedListItem,
            "quote" => BlockType::Quote,
            "divider" => BlockType::Divider,
            _ => BlockType::Paragraph,
        };

        Block {
            block_type,
            content: if self.block_type == "divider" { None } else { Some(content) },
            language,
        }
    }
}

fn plain_text_from_content_json(content_json: &Value) -> String {
    content_json
        .as_array()
        .map(|tokens| {
            tokens
                .iter()
                .filter_map(|token| token.get("plain_text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}
```

- [ ] **Step 4: Wire the module into the app**

Modify `src/lib.rs`.

Add the module export near the other `pub mod` entries:

```rust
pub mod simple_editor;
```

Change `build_app` router assembly from:

```rust
api::router()
    .merge(mcp::router())
    .fallback(static_files::serve)
```

to:

```rust
api::router()
    .merge(mcp::router())
    .merge(simple_editor::router())
    .fallback(static_files::serve)
```

- [ ] **Step 5: Run tests to verify backend passes**

Run:

```bash
cargo test simple_editor_
```

Expected: all three `simple_editor_` tests pass.

- [ ] **Step 6: Commit backend conversion API**

Run:

```bash
git add src/simple_editor.rs src/lib.rs tests/integration.rs
git commit -m "$(cat <<'EOF'
feat: add simple editor conversion API

Expose stateless Markdown/block conversion endpoints for the simple editor without adding server persistence.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Frontend conversion client and editor clearing support

**Files:**
- Create: `web/src/lib/simpleEditorApi.ts`
- Modify: `web/src/TiptapEditor.tsx`

- [ ] **Step 1: Create frontend API client**

Create `web/src/lib/simpleEditorApi.ts`:

```ts
import type { DocumosaBlock } from '@/lib/converter'

async function postJson<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!response.ok) throw new Error(await response.text())
  return response.json() as Promise<T>
}

export async function markdownToBlocks(markdown: string): Promise<DocumosaBlock[]> {
  const response = await postJson<{ blocks: DocumosaBlock[] }>('/simple/editor/md2blocks', { markdown })
  return response.blocks
}

export async function blocksToMarkdown(blocks: DocumosaBlock[]): Promise<string> {
  const response = await postJson<{ markdown: string }>('/simple/editor/blocks2md', { blocks })
  return response.markdown
}
```

- [ ] **Step 2: Update TiptapEditor so empty blocks clear the editor**

In `web/src/TiptapEditor.tsx`, change this effect:

```ts
useEffect(() => {
  if (editor && blocks.length > 0) {
    const currentJson = JSON.stringify(editor.getJSON())
    const newJson = JSON.stringify(blocksToProseMirrorDoc(blocks))
    if (currentJson !== newJson) {
      editor.commands.setContent(blocksToProseMirrorDoc(blocks))
    }
  }
}, [editor, blocks])
```

to:

```ts
useEffect(() => {
  if (!editor) return
  const nextDoc = blocksToProseMirrorDoc(blocks)
  const currentJson = JSON.stringify(editor.getJSON())
  const nextJson = JSON.stringify(nextDoc)
  if (currentJson !== nextJson) {
    editor.commands.setContent(nextDoc)
  }
}, [editor, blocks])
```

- [ ] **Step 3: Run frontend type/build check**

Run:

```bash
npm --prefix web run build
```

Expected: build succeeds.

- [ ] **Step 4: Commit frontend API client support**

Run:

```bash
git add web/src/lib/simpleEditorApi.ts web/src/TiptapEditor.tsx
git commit -m "$(cat <<'EOF'
feat: add simple editor frontend conversion client

Add typed conversion API helpers and allow the shared editor to clear from empty block state.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Simple editor page

**Files:**
- Create: `web/src/SimpleEditorPage.tsx`

- [ ] **Step 1: Create the SimpleEditorPage component**

Create `web/src/SimpleEditorPage.tsx`:

```tsx
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Download, FileUp, RotateCcw } from 'lucide-react'

import TiptapEditor from '@/TiptapEditor'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { blocksToMarkdown, markdownToBlocks } from '@/lib/simpleEditorApi'
import type { DocumosaBlock } from '@/lib/converter'

const DRAFT_KEY = 'documosa.simple_editor.draft'
const DEFAULT_FILENAME = 'documosa-simple-editor.md'

type SimpleEditorDraft = {
  filename: string
  blocks: DocumosaBlock[]
  updatedAt: string
}

function emptyDraft(): SimpleEditorDraft {
  return {
    filename: DEFAULT_FILENAME,
    blocks: [],
    updatedAt: new Date().toISOString(),
  }
}

function loadDraft(): SimpleEditorDraft {
  const raw = localStorage.getItem(DRAFT_KEY)
  if (!raw) return emptyDraft()
  try {
    const parsed = JSON.parse(raw) as Partial<SimpleEditorDraft>
    if (!Array.isArray(parsed.blocks)) return emptyDraft()
    return {
      filename: typeof parsed.filename === 'string' && parsed.filename.trim() ? parsed.filename : DEFAULT_FILENAME,
      blocks: parsed.blocks,
      updatedAt: typeof parsed.updatedAt === 'string' ? parsed.updatedAt : new Date().toISOString(),
    }
  } catch {
    return emptyDraft()
  }
}

function hasContent(blocks: DocumosaBlock[]) {
  return blocks.some((block) => {
    if (block.block_type === 'divider') return true
    try {
      const tokens = JSON.parse(block.content_json) as { plain_text?: string }[]
      return tokens.some((token) => token.plain_text?.trim())
    } catch {
      return false
    }
  })
}

function saveDraft(draft: SimpleEditorDraft) {
  localStorage.setItem(DRAFT_KEY, JSON.stringify(draft))
}

function formatSavedAt(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return 'Not saved yet'
  return `Saved ${date.toLocaleString()}`
}

function downloadText(filename: string, text: string) {
  const blob = new Blob([text], { type: 'text/markdown;charset=utf-8' })
  const href = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = href
  anchor.download = filename.trim() || DEFAULT_FILENAME
  anchor.click()
  URL.revokeObjectURL(href)
}

export default function SimpleEditorPage() {
  const [draft, setDraft] = useState<SimpleEditorDraft>(() => loadDraft())
  const [error, setError] = useState('')
  const [saveError, setSaveError] = useState('')
  const [isConverting, setIsConverting] = useState(false)
  const saveTimer = useRef<number | null>(null)
  const fileInputRef = useRef<HTMLInputElement | null>(null)

  const savedLabel = useMemo(() => {
    if (saveError) return saveError
    return formatSavedAt(draft.updatedAt)
  }, [draft.updatedAt, saveError])

  useEffect(() => {
    return () => {
      if (saveTimer.current !== null) window.clearTimeout(saveTimer.current)
    }
  }, [])

  const persistDraft = useCallback((nextDraft: SimpleEditorDraft) => {
    try {
      saveDraft(nextDraft)
      setSaveError('')
    } catch {
      setSaveError('Auto-save unavailable')
    }
  }, [])

  const updateDraft = useCallback((updater: (current: SimpleEditorDraft) => SimpleEditorDraft, immediate = false) => {
    setDraft((current) => {
      const next = updater(current)
      if (saveTimer.current !== null) window.clearTimeout(saveTimer.current)
      if (immediate) {
        persistDraft(next)
      } else {
        saveTimer.current = window.setTimeout(() => persistDraft(next), 500)
      }
      return next
    })
  }, [persistDraft])

  const handleEditorChange = useCallback((blocks: DocumosaBlock[]) => {
    updateDraft((current) => ({
      ...current,
      blocks,
      updatedAt: new Date().toISOString(),
    }))
  }, [updateDraft])

  const handleFilenameChange = useCallback((filename: string) => {
    updateDraft((current) => ({
      ...current,
      filename,
      updatedAt: new Date().toISOString(),
    }))
  }, [updateDraft])

  async function uploadFile(file: File) {
    if (hasContent(draft.blocks) && !confirm('Uploading will replace the current local draft. Continue?')) return
    setError('')
    setIsConverting(true)
    try {
      const markdown = await file.text()
      const blocks = await markdownToBlocks(markdown)
      const nextDraft = {
        filename: file.name || DEFAULT_FILENAME,
        blocks,
        updatedAt: new Date().toISOString(),
      }
      setDraft(nextDraft)
      persistDraft(nextDraft)
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error))
    } finally {
      setIsConverting(false)
      if (fileInputRef.current) fileInputRef.current.value = ''
    }
  }

  async function downloadDraft() {
    setError('')
    setIsConverting(true)
    try {
      const markdown = await blocksToMarkdown(draft.blocks)
      downloadText(draft.filename || DEFAULT_FILENAME, markdown)
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error))
    } finally {
      setIsConverting(false)
    }
  }

  function clearDraft() {
    if (hasContent(draft.blocks) && !confirm('Clear the current local draft?')) return
    const nextDraft = emptyDraft()
    setDraft(nextDraft)
    localStorage.removeItem(DRAFT_KEY)
    setError('')
    setSaveError('')
  }

  return (
    <main className="min-h-screen bg-background text-foreground flex flex-col">
      <header className="border-b bg-card px-5 py-4 flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div>
          <h1 className="text-xl font-semibold">Simple Markdown Editor</h1>
          <p className="text-sm text-muted-foreground">Upload, edit, auto-save locally, and download Markdown.</p>
        </div>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
          <Input
            value={draft.filename}
            onChange={(event) => handleFilenameChange(event.target.value)}
            aria-label="Filename"
            className="sm:w-64"
          />
          <input
            ref={fileInputRef}
            type="file"
            accept=".md,.markdown,text/markdown,text/plain"
            className="hidden"
            onChange={(event) => {
              const file = event.target.files?.[0]
              if (file) void uploadFile(file)
            }}
          />
          <Button type="button" variant="outline" disabled={isConverting} onClick={() => fileInputRef.current?.click()}>
            <FileUp className="h-4 w-4 mr-1.5" />
            Upload
          </Button>
          <Button type="button" disabled={isConverting} onClick={() => void downloadDraft()}>
            <Download className="h-4 w-4 mr-1.5" />
            Download
          </Button>
          <Button type="button" variant="outline" disabled={isConverting} onClick={clearDraft}>
            <RotateCcw className="h-4 w-4 mr-1.5" />
            Clear
          </Button>
        </div>
      </header>

      <section className="border-b px-5 py-2 text-xs text-muted-foreground flex items-center justify-between gap-3">
        <span>{savedLabel}</span>
        {isConverting ? <span>Converting...</span> : null}
      </section>

      {error ? (
        <Alert className="m-5 mb-0">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}

      <section className="flex-1 min-h-0 p-5 overflow-auto">
        <div className="min-h-[70vh] rounded-lg border bg-card px-5 py-4">
          <TiptapEditor
            blocks={draft.blocks}
            readOnly={false}
            onChange={handleEditorChange}
          />
        </div>
      </section>
    </main>
  )
}
```

- [ ] **Step 2: Run frontend build**

Run:

```bash
npm --prefix web run build
```

Expected: build succeeds. If TypeScript reports that `content_json` is not assignable because the API returns JSON arrays, change `SimpleEditorBlock.content_json` in `src/simple_editor.rs` to serialize as a string instead, and update tests to assert `serde_json::from_str::<Value>(blocks[0]["content_json"].as_str().unwrap())`.

- [ ] **Step 3: Commit simple editor page**

Run:

```bash
git add web/src/SimpleEditorPage.tsx
git commit -m "$(cat <<'EOF'
feat: add simple markdown editor page

Add a local-draft editor UI for uploading, editing, clearing, and downloading Markdown.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Frontend route selection

**Files:**
- Modify: `web/src/main.tsx`

- [ ] **Step 1: Route `/simple/editor` to the simple editor page**

Change `web/src/main.tsx` from:

```tsx
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import './index.css'
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
```

to:

```tsx
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import './index.css'
import App from './App.tsx'
import SimpleEditorPage from './SimpleEditorPage.tsx'

const Root = window.location.pathname === '/simple/editor' ? SimpleEditorPage : App

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
```

- [ ] **Step 2: Run frontend build**

Run:

```bash
npm --prefix web run build
```

Expected: build succeeds.

- [ ] **Step 3: Commit route selection**

Run:

```bash
git add web/src/main.tsx
git commit -m "$(cat <<'EOF'
feat: route simple editor page

Render the local Markdown editor at /simple/editor while preserving the main app elsewhere.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: End-to-end verification and polish

**Files:**
- Modify only if verification exposes a concrete bug.

- [ ] **Step 1: Run full Rust verification**

Run:

```bash
cargo test simple_editor_ && cargo test && cargo build
```

Expected: all tests pass and build succeeds.

- [ ] **Step 2: Run frontend verification**

Run:

```bash
npm --prefix web run build
```

Expected: TypeScript and Vite build succeed.

- [ ] **Step 3: Start the app for browser testing**

Run:

```bash
cargo run -- serve --addr 127.0.0.1 --port 4317 --data-dir /tmp/documosa-simple-editor-check
```

Expected: server logs a local URL, usually `http://127.0.0.1:4317` or the next free port.

- [ ] **Step 4: Browser-test the golden path**

Open the served URL plus `/simple/editor`, for example:

```text
http://127.0.0.1:4317/simple/editor
```

Manual test:

1. Upload a local file containing:

   ```markdown
   # Browser Test

   - one
   - two
   ```

2. Confirm the editor shows `Browser Test`, `one`, and `two`.
3. Type additional text in the editor.
4. Refresh the page and confirm the draft restores.
5. Click Download and confirm a `.md` file downloads.
6. Click Clear, confirm, refresh, and confirm the editor stays empty.
7. Open browser console and confirm there are no uncaught errors.

- [ ] **Step 5: Verify main app still loads**

Open the served root URL:

```text
http://127.0.0.1:4317/
```

Expected: existing Documosa main UI loads, not `SimpleEditorPage`.

- [ ] **Step 6: Commit verification fixes if any**

If Step 4 or Step 5 required code changes, commit only those changes:

```bash
git add <changed-files>
git commit -m "$(cat <<'EOF'
fix: polish simple editor runtime flow

Resolve browser-test issues found in the simple Markdown editor flow.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

If no fixes were needed, do not create an empty commit.

---

## Self-Review

- Spec coverage: Tasks cover the route, stateless conversion APIs, editor reuse, single localStorage draft, upload/download/clear, debounce auto-save, error handling, build checks, and browser checks.
- Placeholder scan: No TBD/TODO placeholders remain. The only conditional note is a concrete TypeScript contingency with an explicit fix.
- Type consistency: Backend `SimpleEditorBlock` is the API shape used by `DocumosaBlock`; frontend client returns `DocumosaBlock[]`; `SimpleEditorPage` consumes `DocumosaBlock[]`; `TiptapEditor` already consumes `DocumosaBlock[]`.
