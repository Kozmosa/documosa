# Documosa

Documosa is a trusted-LAN collaborative plain-text document tool. One Rust process serves the REST API, WebSocket document events, Streamable HTTP MCP endpoint, CLI-backed operations, and production web assets.

## Run

```sh
cargo run -- serve --addr 0.0.0.0 --port 4317 --data-dir ~/.documosa
```

If the requested port is already occupied, `serve` automatically tries the next port until it finds an available one.

The SQLite database is stored at `~/.documosa/documosa.sqlite` by default. Build the web assets before using the served browser UI:

```sh
cd web
npm install
npm run build
```

For frontend development:

```sh
cd web
npm run dev
```

## Identity And Modes

Documosa v1 assumes a trusted LAN. It does not provide password authentication. Clients identify with a durable client ID, nickname, and selected operation mode.

Reviewer and writer are operation modes, not account roles. Users can switch freely. Reviewer mode can comment, reply, and suggest. Writer mode can edit lines, resolve comments, and accept or reject suggestions.

## CLI Examples

```sh
documosa document create --server http://127.0.0.1:4317 --role-mode writer --title "Draft" --content "line one"
documosa document list --server http://127.0.0.1:4317
documosa line insert --server http://127.0.0.1:4317 --role-mode writer <document-id> --line "new line"
documosa document export --server http://127.0.0.1:4317 <document-id>
documosa history list --server http://127.0.0.1:4317 <document-id> --category all
documosa history diff --server http://127.0.0.1:4317 <document-id> --from <audit-event-id> --to <audit-event-id>
documosa history note set --server http://127.0.0.1:4317 <document-id> <audit-event-id> --body "shared note"
```

CLI commands call the backend API and never write SQLite directly.

## MCP

The Streamable HTTP MCP endpoint is `POST /mcp`. It supports `initialize`, `tools/list`, and `tools/call`. Tools include document list/create/get/export, line insert/replace/delete, comments, replies, resolution, suggestions, and suggestion decisions.

History tools include `list_history_events`, `diff_history_events`, `set_audit_event_note`, and `clear_audit_event_note`. Tool results include both `structuredContent` and text `content`; `diff_history_events` returns a unified diff in the text content and in `structuredContent.unified_diff`.

## Verification

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cd web
npm run build
npm run test:e2e
```
