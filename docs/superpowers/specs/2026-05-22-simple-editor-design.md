# Simple Markdown Editor Design

## Context

Documosa needs a lightweight editor at `/simple/editor` for quick Markdown file editing. This editor should not create pages, use collaboration features, or persist data on the server. It should reuse the existing WYSIWYG editor component while keeping drafts in the browser so users can upload, edit, refresh safely, and download Markdown files.

## Goals

- Provide a simple `/simple/editor` route in the existing served web app.
- Reuse the current `TiptapEditor` component for WYSIWYG editing.
- Let users upload a Markdown file, edit it, and download the edited Markdown.
- Auto-save one local draft in browser storage with a debounce.
- Reuse the existing Rust Markdown/block conversion logic through stateless HTTP APIs.

## Non-goals

- No database persistence for simple editor documents.
- No comments, suggestions, locks, presence, history, or collaboration features.
- No multi-document local library in the first version.
- No separate frontend Markdown parser/serializer.
- No guarantee of perfect Markdown round-trip beyond what the existing block conversion and editor support.

## Routing and architecture

The frontend entry point will choose which app to render based on `window.location.pathname`:

- `/simple/editor` renders a new `SimpleEditorPage`.
- All other paths continue to render the existing main `App`.

This avoids adding a router dependency and keeps the simple editor separate from the main document workspace state.

The backend will expose two stateless conversion endpoints:

- `POST /simple/editor/md2blocks`
- `POST /simple/editor/blocks2md`

These endpoints do not read or write the database and do not require JWT. They only convert payloads and return converted content.

## Backend conversion API

### `POST /simple/editor/md2blocks`

Request:

```json
{ "markdown": "# Title\n\nBody" }
```

Response:

```json
{ "blocks": [] }
```

The handler will call `mmdash_blocks::markdown_to_blocks` and return the resulting blocks.

### `POST /simple/editor/blocks2md`

Request:

```json
{ "blocks": [] }
```

Response:

```json
{ "markdown": "# Title\n\nBody" }
```

The handler will call `mmdash_blocks::blocks_to_markdown` and return the resulting Markdown.

The block shape should match the existing frontend `DocumosaBlock` needs: `block_type`, `content_json`, optional `properties_json`, and optional local `id` if present. The simple editor does not need database IDs.

## Frontend page

Add `web/src/SimpleEditorPage.tsx`.

The page state is a single local draft:

```ts
{
  filename: string
  blocks: DocumosaBlock[]
  updatedAt: string
}
```

Use one storage key, for example `documosa.simple_editor.draft`.

The page layout is intentionally minimal:

- Header/tool row:
  - page title
  - filename input
  - upload button/file input
  - download button
  - clear button
  - auto-save status text
- Main content:
  - `TiptapEditor` in editable mode
- Alert/status area:
  - upload, conversion, download, or storage errors

## Data flow

### Initial load

1. Read the draft JSON from local storage.
2. If valid, restore `filename`, `blocks`, and `updatedAt`.
3. If missing or invalid, start with an empty untitled draft.

### Upload

1. User selects a Markdown-like file.
2. If the current draft has content, confirm that uploading will replace it.
3. Read the selected file as text.
4. Call `POST /simple/editor/md2blocks`.
5. Replace editor blocks with the response.
6. Set filename to the uploaded filename.
7. Save the new draft immediately.

The file input should accept `.md`, `.markdown`, and `text/markdown`, but the UI should not hard-block unusual extensions because Markdown files may be extensionless.

### Edit and auto-save

`TiptapEditor.onChange` updates the in-memory blocks. A debounce, around 500ms, writes the draft to local storage and updates `updatedAt`.

If local storage fails, the editor remains usable and the page displays an auto-save error.

### Download

1. Call `POST /simple/editor/blocks2md` with current blocks.
2. Create a Blob from the returned Markdown.
3. Download using the current filename.
4. If no filename exists, use `documosa-simple-editor.md`.

Downloading does not clear the local draft.

### Clear

The clear button confirms with the user, then removes the local draft and resets filename and blocks.

## Error handling

- Invalid saved draft: ignore it and start empty.
- Upload read failure: keep the current draft and show an error.
- Conversion API failure: keep the current draft and show an error.
- Download conversion failure: keep the current draft and show an error.
- Local storage failure: continue editing and show that auto-save is unavailable.

## Testing and verification

Backend tests:

- `md2blocks` converts representative Markdown into blocks.
- `blocks2md` converts representative blocks into Markdown.
- malformed JSON returns an appropriate client error through existing Axum JSON handling.

Frontend/build verification:

- `npm --prefix web run build` succeeds.
- The simple editor route is included in the served SPA fallback.

Manual browser verification:

1. Open `/simple/editor`.
2. Upload a Markdown file.
3. Confirm the WYSIWYG editor displays content.
4. Edit content and refresh; confirm local draft restores.
5. Download the edited file and inspect Markdown output.
6. Upload another file and confirm replacement prompt appears.
7. Clear the draft and refresh; confirm it stays empty.
