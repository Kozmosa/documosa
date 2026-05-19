use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use documosa::models::*;
use documosa::db::{BlockInput, CommentDraft};
use reqwest::Client;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

// ─── helper functions ───

fn actor(client_id: &str, role_mode: RoleMode) -> Identity {
    Identity {
        client_id: client_id.to_string(),
        nickname: client_id.to_string(),
        role_mode,
        actor_kind: Default::default(),
    }
}

async fn pool() -> sqlx::SqlitePool {
    let pool = documosa::db::connect_memory().await.unwrap();
    documosa::db::migrate(&pool).await.unwrap();
    pool
}

async fn file_pool(temp_dir: &TempDir) -> sqlx::SqlitePool {
    let pool = documosa::db::connect(&temp_dir.path().join("documosa.sqlite"))
        .await
        .unwrap();
    documosa::db::migrate(&pool).await.unwrap();
    pool
}

fn audit_details(snapshot: &PageSnapshot, event_type: &str) -> Value {
    let event = snapshot
        .audit_events
        .iter()
        .find(|event| event.event_type == event_type)
        .unwrap_or_else(|| panic!("missing audit event {event_type}"));
    serde_json::from_str(&event.details_json).unwrap()
}

// Build rich-text JSON content for a plain text string
fn rich_text_json(text: &str) -> String {
    serde_json::to_string(&[json!({
        "type": "text",
        "text": { "content": text },
        "plain_text": text,
    })])
    .unwrap()
}

// Build blocks JSON for create_page from plain text strings
fn make_blocks_json(texts: &[&str]) -> String {
    let inputs: Vec<Value> = texts
        .iter()
        .map(|t| {
            json!({
                "block_type": "paragraph",
                "content_json": rich_text_json(t),
            })
        })
        .collect();
    serde_json::to_string(&inputs).unwrap()
}

// Extract plain text from a block's content_json
fn block_text(block: &Block) -> String {
    let tokens: Vec<Value> = serde_json::from_str(&block.content_json).unwrap_or_default();
    tokens
        .iter()
        .filter_map(|t| t.get("plain_text").and_then(|v| v.as_str()))
        .collect::<Vec<&str>>()
        .join("")
}

// Extract plain texts from multiple blocks
fn block_texts(blocks: &[Block]) -> Vec<String> {
    blocks.iter().map(block_text).collect()
}

// Helper to create a single BlockInput
fn block_input(text: &str) -> BlockInput {
    BlockInput {
        block_type: "paragraph".to_string(),
        content_json: rich_text_json(text),
        properties_json: None,
    }
}

// ─── MCP test helpers ───

async fn mcp_rpc(app: axum::Router, payload: Value) -> Value {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn mcp_tool(app: axum::Router, id: i64, name: &str, arguments: Value) -> Value {
    mcp_rpc(
        app,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        }),
    )
    .await
}

// ─── Notion mock helpers ───

#[derive(Clone, Default)]
struct NotionMockState {
    requests: Arc<Mutex<Vec<RecordedNotionRequest>>>,
}

#[derive(Debug, Clone)]
struct RecordedNotionRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    notion_version: Option<String>,
    body: Value,
}

fn spawn_notion_mock() -> (String, NotionMockState) {
    let state = NotionMockState::default();
    let app = Router::new()
        .route("/{*path}", any(notion_mock_handler))
        .with_state(state.clone());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    (base_url, state)
}

async fn recorded_notion_requests(state: &NotionMockState) -> Vec<RecordedNotionRequest> {
    state.requests.lock().await.clone()
}

async fn notion_mock_handler(
    State(state): State<NotionMockState>,
    request: Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let method = parts.method.clone();
    let path = parts.uri.path().to_string();
    let authorization = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let notion_version = parts
        .headers
        .get("Notion-Version")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    state.requests.lock().await.push(RecordedNotionRequest {
        method: method.to_string(),
        path: path.clone(),
        authorization,
        notion_version,
        body,
    });

    match (method, path.as_str()) {
        (Method::GET, "/v1/blocks/parent-page/children") => Json(json!({
            "results": [
                notion_child_page("page-1", "Listed Doc"),
                {
                    "id": "heading-1",
                    "type": "heading_1",
                    "heading_1": { "rich_text": notion_text("Ignored") }
                }
            ],
            "has_more": false,
            "next_cursor": null
        }))
        .into_response(),
        (Method::GET, "/v1/pages/page-1") => {
            Json(notion_page("page-1", "Notion Doc")).into_response()
        }
        (Method::GET, "/v1/blocks/page-1/children") => Json(json!({
            "results": [
                notion_paragraph("line-1", "one"),
                {
                    "id": "heading-1",
                    "created_time": "2026-04-28T00:00:00.000Z",
                    "last_edited_time": "2026-04-28T00:00:00.000Z",
                    "type": "heading_1",
                    "heading_1": { "rich_text": notion_text("Ignored") }
                },
                notion_paragraph("line-2", "two")
            ],
            "has_more": false,
            "next_cursor": null
        }))
        .into_response(),
        (Method::POST, "/v1/pages") => Json(notion_page("page-1", "Notion Doc")).into_response(),
        (Method::PATCH, "/v1/blocks/page-1/children") => {
            Json(json!({ "object": "list", "results": [] })).into_response()
        }
        (Method::PATCH, "/v1/blocks/line-1") => {
            Json(notion_paragraph("line-1", "ONE")).into_response()
        }
        (Method::DELETE, "/v1/blocks/line-1") => {
            Json(notion_paragraph("line-1", "one")).into_response()
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "message": "not found" })),
        )
            .into_response(),
    }
}

fn notion_page(id: &str, title: &str) -> Value {
    json!({
        "id": id,
        "created_time": "2026-04-28T00:00:00.000Z",
        "last_edited_time": "2026-04-28T00:00:00.000Z",
        "properties": {
            "title": {
                "type": "title",
                "title": notion_text(title)
            }
        }
    })
}

fn notion_child_page(id: &str, title: &str) -> Value {
    json!({
        "id": id,
        "created_time": "2026-04-28T00:00:00.000Z",
        "last_edited_time": "2026-04-28T00:00:00.000Z",
        "type": "child_page",
        "child_page": { "title": title }
    })
}

fn notion_paragraph(id: &str, content: &str) -> Value {
    json!({
        "id": id,
        "created_time": "2026-04-28T00:00:00.000Z",
        "last_edited_time": "2026-04-28T00:00:00.000Z",
        "type": "paragraph",
        "paragraph": { "rich_text": notion_text(content) }
    })
}

fn notion_text(content: &str) -> Value {
    json!([
        {
            "type": "text",
            "text": { "content": content },
            "plain_text": content
        }
    ])
}

// ─── JWT helpers (for API tests) ───

const MMDASH_JWT_SECRET: &str = "test-jwt-secret-shared";

fn ensure_jwt_secret() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        std::env::set_var("JWT_SECRET", MMDASH_JWT_SECRET);
    });
}

fn mmdash_token(sub: &str, name: Option<&str>) -> String {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use serde::Serialize;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Serialize)]
    struct Claims {
        sub: String,
        name: Option<String>,
        email: Option<String>,
        exp: i64,
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let claims = Claims {
        sub: sub.into(),
        name: name.map(Into::into),
        email: None,
        exp: now + 3600,
    };
    let header = Header::new(Algorithm::HS256);
    encode(
        &header,
        &claims,
        &EncodingKey::from_secret(MMDASH_JWT_SECRET.as_bytes()),
    )
    .unwrap()
}

// ─── Tests ───

#[tokio::test]
async fn page_blocks_locks_comments_suggestions_and_audit_work() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);
    let reviewer = actor("reviewer", RoleMode::Reviewer);
    let other = actor("other", RoleMode::Writer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one", "two"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id;
    let first = created.blocks[0].id.clone();
    let second = created.blocks[1].id.clone();

    assert_eq!(block_texts(&created.blocks), vec!["one", "two"]);

    // Update first block
    let edited = documosa::db::update_block(
        &pool,
        &writer,
        &first,
        None,
        Some(&rich_text_json("ONE")),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        block_text(edited.blocks.iter().find(|b| b.id == first).unwrap()),
        "ONE"
    );

    // Heartbeat locks on second block
    documosa::db::heartbeat_locks(&pool, &other, &page_id, vec![second.clone()])
        .await
        .unwrap();
    let locked = documosa::db::snapshot(&pool, &page_id).await.unwrap();
    assert!(locked.locks.iter().any(|l| l.block_id == second));

    // Expire locks
    sqlx::query("UPDATE block_locks SET expires_at = '2000-01-01T00:00:00Z'")
        .execute(&pool)
        .await
        .unwrap();

    // Update second block after lock expired
    documosa::db::update_block(
        &pool,
        &writer,
        &second,
        None,
        Some(&rich_text_json("TWO")),
        None,
    )
    .await
    .unwrap();

    // Comment CRUD
    let commented = documosa::db::create_comment(
        &pool,
        &reviewer,
        &page_id,
        CommentDraft {
            target_block_id: first.clone(),
            start_column: None,
            end_column: None,
            body: "Needs tone pass".to_string(),
        },
    )
    .await
    .unwrap();
    let comment_id = commented.comments[0].id.clone();
    documosa::db::reply_comment(&pool, &writer, &page_id, &comment_id, "Done".to_string())
        .await
        .unwrap();
    documosa::db::resolve_comment(&pool, &writer, &page_id, &comment_id)
        .await
        .unwrap();

    // Suggestion create + accept with conflict
    let suggested = documosa::db::create_suggestion(
        &pool,
        &reviewer,
        &page_id,
        "replace".to_string(),
        Some(first.clone()),
        vec!["one revised".to_string()],
    )
    .await
    .unwrap();
    let suggestion_id = suggested.suggestions[0].id.clone();

    // Update the block to create a base revision conflict
    documosa::db::update_block(
        &pool,
        &writer,
        &first,
        None,
        Some(&rich_text_json("conflicting edit")),
        None,
    )
    .await
    .unwrap();

    let suggestion_conflict =
        documosa::db::decide_suggestion(&pool, &writer, &page_id, &suggestion_id, true).await;
    assert!(suggestion_conflict.is_err());

    // Verify audit events have both roles
    let audit = documosa::db::snapshot(&pool, &page_id)
        .await
        .unwrap()
        .audit_events;
    assert!(audit.iter().any(|event| event.role_mode == "reviewer"));
    assert!(audit.iter().any(|event| event.role_mode == "writer"));
}

#[tokio::test]
async fn block_audit_details_store_summaries_counts_and_block_ids() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);
    let long = "a".repeat(140);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&[&long, "short"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();
    let first_id = created.blocks[0].id.clone();

    let created_details = audit_details(&created, "page.created");
    assert_eq!(created_details["title"], "Draft");

    // Append a block
    let appended = documosa::db::append_blocks(
        &pool,
        &writer,
        &page_id,
        vec![BlockInput {
            block_type: "paragraph".to_string(),
            content_json: rich_text_json(&"b".repeat(130)),
            properties_json: None,
        }],
        Some(&first_id),
    )
    .await
    .unwrap();
    let append_details = audit_details(&appended, "blocks.appended");
    assert_eq!(append_details["count"], 1);
    let appended_id = append_details["block_ids"][0].as_str().unwrap().to_string();

    // Update first block
    let updated = documosa::db::update_block(
        &pool,
        &writer,
        &first_id,
        None,
        Some(&rich_text_json(&"c".repeat(125))),
        None,
    )
    .await
    .unwrap();
    let update_details = audit_details(&updated, "block.updated");
    assert_eq!(update_details["block_id"], first_id);
    assert_eq!(update_details["before_block_type"], "paragraph");

    // Delete the appended block
    let deleted = documosa::db::delete_block(&pool, &writer, &appended_id)
        .await
        .unwrap();
    let delete_details = audit_details(&deleted, "block.deleted");
    assert_eq!(delete_details["block_id"], appended_id);
    assert_eq!(delete_details["block_type"], "paragraph");
}

#[tokio::test]
async fn inserting_blocks_without_anchor_places_blocks_at_end() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one", "two"]),
    )
    .await
    .unwrap();

    let appended = documosa::db::append_blocks(
        &pool,
        &writer,
        &created.page.id,
        vec![block_input("zero-a"), block_input("zero-b")],
        None,
    )
    .await
    .unwrap();

    assert_eq!(block_texts(&appended.blocks), vec!["one", "two", "zero-a", "zero-b"]);
}

#[tokio::test]
async fn inserting_blocks_after_anchor_keeps_stable_order() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one", "two"]),
    )
    .await
    .unwrap();
    let first_id = created.blocks[0].id.clone();

    let appended = documosa::db::append_blocks(
        &pool,
        &writer,
        &created.page.id,
        vec![block_input("one-a"), block_input("one-b")],
        Some(&first_id),
    )
    .await
    .unwrap();

    assert_eq!(
        block_texts(&appended.blocks),
        vec!["one", "one-a", "one-b", "two"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_file_backed_append_blocks_do_not_fail_with_database_locked() {
    let temp_dir = tempfile::tempdir().unwrap();
    let pool = file_pool(&temp_dir).await;
    let writer = actor("writer", RoleMode::Writer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Concurrent".to_string(),
        make_blocks_json(
            &(1..=100)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        ),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();

    let mut handles = Vec::new();
    for index in 0..16 {
        let pool = pool.clone();
        let page_id = page_id.clone();
        handles.push(tokio::spawn(async move {
            let writer = actor(&format!("writer-{index}"), RoleMode::Writer);
            documosa::db::append_blocks(
                &pool,
                &writer,
                &page_id,
                vec![BlockInput {
                    block_type: "paragraph".to_string(),
                    content_json: rich_text_json(&format!("concurrent {index}")),
                    properties_json: None,
                }],
                None,
            )
            .await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        if let Err(error) = &result {
            assert!(
                !error.to_string().contains("database is locked"),
                "unexpected SQLite lock failure: {error}"
            );
        }
        result.unwrap();
    }

    let snapshot = documosa::db::snapshot(&pool, &page_id).await.unwrap();
    assert_eq!(snapshot.blocks.len(), 116);
    assert!(
        snapshot
            .audit_events
            .iter()
            .filter(|event| event.event_type == "blocks.appended")
            .count()
            >= 16
    );
}

#[tokio::test]
async fn comment_audit_details_store_full_comment_bodies() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);
    let reviewer = actor("reviewer", RoleMode::Reviewer);
    let full_body = format!("{} tail", "comment body ".repeat(40));
    let reply_body = format!("{} reply", "reply body ".repeat(35));
    let updated_body = format!("{} updated", "updated body ".repeat(35));

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();
    let block_id = created.blocks[0].id.clone();

    let commented = documosa::db::create_comment(
        &pool,
        &reviewer,
        &page_id,
        CommentDraft {
            target_block_id: block_id.clone(),
            start_column: Some(0),
            end_column: Some(3),
            body: full_body.clone(),
        },
    )
    .await
    .unwrap();
    let comment_id = commented.comments[0].id.clone();
    let create_details = audit_details(&commented, "comment.created");
    assert_eq!(create_details["comment_id"], comment_id);
    assert_eq!(create_details["target_block_id"], block_id);
    assert_eq!(create_details["start_column"], 0);
    assert_eq!(create_details["end_column"], 3);
    assert_eq!(create_details["body"], full_body);

    let updated = documosa::db::update_comment(
        &pool,
        &reviewer,
        &page_id,
        &comment_id,
        updated_body.clone(),
    )
    .await
    .unwrap();
    let update_details = audit_details(&updated, "comment.updated");
    assert_eq!(update_details["before_body"], full_body);
    assert_eq!(update_details["after_body"], updated_body);

    let replied = documosa::db::reply_comment(
        &pool,
        &writer,
        &page_id,
        &comment_id,
        reply_body.clone(),
    )
    .await
    .unwrap();
    let reply_details = audit_details(&replied, "comment.replied");
    assert_eq!(reply_details["body"], reply_body);
    assert_eq!(reply_details["comment_id"], comment_id);

    let resolved = documosa::db::resolve_comment(&pool, &writer, &page_id, &comment_id)
        .await
        .unwrap();
    let resolve_details = audit_details(&resolved, "comment.resolved");
    assert_eq!(resolve_details["body"], updated_body);

    let deleted = documosa::db::delete_comment(&pool, &writer, &page_id, &comment_id)
        .await
        .unwrap();
    let delete_details = audit_details(&deleted, "comment.deleted");
    assert_eq!(delete_details["body"], updated_body);
}

#[tokio::test]
async fn audit_event_notes_are_shared_snapshot_data_without_extra_audit_noise() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);
    let reviewer = actor("reviewer", RoleMode::Reviewer);
    let other_writer = actor("other-writer", RoleMode::Writer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();
    let audit_event_id = created.audit_events[0].id.clone();
    let audit_count = created.audit_events.len();

    let noted = documosa::db::put_audit_event_note(
        &pool,
        &reviewer,
        &page_id,
        &audit_event_id,
        " shared note ".to_string(),
    )
    .await
    .unwrap();
    let noted_event = noted
        .audit_events
        .iter()
        .find(|event| event.id == audit_event_id)
        .unwrap();
    assert_eq!(noted_event.note_body.as_deref(), Some("shared note"));
    assert_eq!(
        noted_event.note_updated_by_nickname.as_deref(),
        Some("reviewer")
    );
    assert!(noted_event.note_updated_at.is_some());
    assert_eq!(noted.audit_events.len(), audit_count);

    let cleared = documosa::db::put_audit_event_note(
        &pool,
        &writer,
        &page_id,
        &audit_event_id,
        "   ".to_string(),
    )
    .await
    .unwrap();
    let cleared_event = cleared
        .audit_events
        .iter()
        .find(|event| event.id == audit_event_id)
        .unwrap();
    assert_eq!(cleared_event.note_body, None);
    assert_eq!(cleared.audit_events.len(), audit_count);

    // Cross-document note attempt (wrong page_id)
    let other = documosa::db::create_page(
        &pool,
        &other_writer,
        "Other".to_string(),
        make_blocks_json(&["two"]),
    )
    .await
    .unwrap();
    let cross_result = documosa::db::put_audit_event_note(
        &pool,
        &reviewer,
        &other.page.id,
        &audit_event_id,
        "wrong document".to_string(),
    )
    .await;
    assert!(cross_result.is_err());
}

#[tokio::test]
async fn history_diff_uses_stored_page_versions_and_conflicts_when_missing() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);
    let reviewer = actor("reviewer", RoleMode::Reviewer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();
    let created_event_id = created
        .audit_events
        .iter()
        .find(|event| event.event_type == "page.created")
        .unwrap()
        .id
        .clone();

    let appended = documosa::db::append_blocks(
        &pool,
        &writer,
        &page_id,
        vec![block_input("two")],
        Some(&created.blocks[0].id),
    )
    .await
    .unwrap();
    let appended_event_id = appended
        .audit_events
        .iter()
        .find(|event| event.event_type == "blocks.appended")
        .unwrap()
        .id
        .clone();

    let diff =
        documosa::db::history_diff(&pool, &page_id, &created_event_id, &appended_event_id)
            .await
            .unwrap();
    assert_eq!(diff.from_content, "one");
    assert_eq!(diff.to_content, "one\ntwo");

    // Comment-only event should have same content
    let commented = documosa::db::create_comment(
        &pool,
        &reviewer,
        &page_id,
        CommentDraft {
            target_block_id: created.blocks[0].id.clone(),
            start_column: None,
            end_column: None,
            body: "note".to_string(),
        },
    )
    .await
    .unwrap();
    let comment_event_id = commented
        .audit_events
        .iter()
        .find(|event| event.event_type == "comment.created")
        .unwrap()
        .id
        .clone();
    let non_content_diff =
        documosa::db::history_diff(&pool, &page_id, &appended_event_id, &comment_event_id)
            .await
            .unwrap();
    assert_eq!(non_content_diff.from_content, "one\ntwo");
    assert_eq!(non_content_diff.to_content, "one\ntwo");

    // Legacy event with no version data
    let legacy_event_id = new_id();
    sqlx::query("INSERT INTO audit_events (id, page_id, actor_client_id, actor_nickname, role_mode, event_type, details_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&legacy_event_id)
        .bind(&page_id)
        .bind("legacy")
        .bind("Legacy")
        .bind("writer")
        .bind("page.content_updated")
        .bind("{}")
        .bind(now())
        .execute(&pool)
        .await
        .unwrap();
    let missing =
        documosa::db::history_diff(&pool, &page_id, &appended_event_id, &legacy_event_id)
            .await
            .unwrap_err();
    assert_eq!(missing.to_string(), "version data not available");
}

#[tokio::test]
async fn history_diff_keeps_literal_diff_prefix_lines_agent_readable() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Diff Prefixes".to_string(),
        make_blocks_json(&["+literal", "-unchanged", "plain"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();
    let first_block_id = created.blocks[0].id.clone();
    let created_event_id = created
        .audit_events
        .iter()
        .find(|event| event.event_type == "page.created")
        .unwrap()
        .id
        .clone();

    let updated = documosa::db::update_block(
        &pool,
        &writer,
        &first_block_id,
        None,
        Some(&rich_text_json("+literal changed")),
        None,
    )
    .await
    .unwrap();
    let updated_event_id = updated
        .audit_events
        .iter()
        .find(|event| event.event_type == "block.updated")
        .unwrap()
        .id
        .clone();

    let history_diff =
        documosa::db::history_diff(&pool, &page_id, &created_event_id, &updated_event_id)
            .await
            .unwrap();
    let formatted = documosa::diff::format_history_unified_diff(&history_diff, 1);

    assert!(formatted.contains("--- "));
    assert!(formatted.contains("+++ "));
    assert!(formatted.contains("@@"));
    assert!(formatted.contains("-+literal\n"));
    assert!(formatted.contains("++literal changed\n"));
    assert!(formatted.contains(" -unchanged\n"));
    assert!(!formatted.contains("--unchanged\n"));
}

#[tokio::test]
async fn history_api_lists_and_filters_events() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;
    let writer = actor("writer", RoleMode::Writer);
    let reviewer = actor("reviewer", RoleMode::Reviewer);
    let token = mmdash_token("history-test", Some("Tester"));

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();
    let first_block_id = created.blocks[0].id.clone();
    documosa::db::append_blocks(
        &pool,
        &writer,
        &page_id,
        vec![block_input("two")],
        Some(&first_block_id),
    )
    .await
    .unwrap();
    documosa::db::create_comment(
        &pool,
        &reviewer,
        &page_id,
        CommentDraft {
            target_block_id: first_block_id.clone(),
            start_column: None,
            end_column: None,
            body: "note".to_string(),
        },
    )
    .await
    .unwrap();
    documosa::db::create_suggestion(
        &pool,
        &reviewer,
        &page_id,
        "replace".to_string(),
        Some(first_block_id.clone()),
        vec!["ONE".to_string()],
    )
    .await
    .unwrap();
    documosa::db::heartbeat_locks(&pool, &writer, &page_id, vec![first_block_id])
        .await
        .unwrap();

    // Helper to make authenticated GET requests
    async fn get_json_auth(app: axum::Router, uri: &str, token: &str) -> Value {
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    let all = get_json_auth(
        app.clone(),
        &format!("/pages/{page_id}/history?category=all&limit=10"),
        &token,
    )
    .await;
    let all_events = all.as_array().unwrap();
    assert_eq!(all_events.len(), 5);
    assert!(
        all_events
            .iter()
            .any(|event| event["event_type"] == "suggestion.created")
    );
    assert!(
        all_events
            .iter()
            .any(|event| event["event_type"] == "locks.heartbeat")
    );

    let default = get_json_auth(
        app.clone(),
        &format!("/pages/{page_id}/history"),
        &token,
    )
    .await;
    let default_event_types: Vec<_> = default
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["event_type"].as_str().unwrap())
        .collect();
    assert!(default_event_types.contains(&"page.created"));
    assert!(default_event_types.contains(&"blocks.appended"));
    assert!(default_event_types.contains(&"comment.created"));
    assert!(!default_event_types.contains(&"suggestion.created"));
    assert!(!default_event_types.contains(&"locks.heartbeat"));

    let limited = get_json_auth(
        app.clone(),
        &format!("/pages/{page_id}/history?category=all&limit=2"),
        &token,
    )
    .await;
    assert_eq!(limited.as_array().unwrap().len(), 2);

    let suggestions = get_json_auth(
        app.clone(),
        &format!("/pages/{page_id}/history?category=suggestion"),
        &token,
    )
    .await;
    assert_eq!(suggestions.as_array().unwrap().len(), 1);
    assert_eq!(
        suggestions.as_array().unwrap()[0]["event_type"],
        "suggestion.created"
    );

    let system = get_json_auth(
        app.clone(),
        &format!("/pages/{page_id}/history?category=system"),
        &token,
    )
    .await;
    assert_eq!(system.as_array().unwrap().len(), 1);
    assert_eq!(
        system.as_array().unwrap()[0]["event_type"],
        "locks.heartbeat"
    );

    let suggestion_time = all_events
        .iter()
        .find(|event| event["event_type"] == "suggestion.created")
        .unwrap()["created_at"]
        .as_str()
        .unwrap()
        .replace('+', "%2B");
    let from_suggestion = get_json_auth(
        app.clone(),
        &format!(
            "/pages/{page_id}/history?category=all&from={suggestion_time}"
        ),
        &token,
    )
    .await;
    let from_event_types: Vec<_> = from_suggestion
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["event_type"].as_str().unwrap())
        .collect();
    assert!(from_event_types.contains(&"suggestion.created"));
    assert!(from_event_types.contains(&"locks.heartbeat"));
    assert!(!from_event_types.contains(&"comment.created"));

    let bad_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/pages/{page_id}/history?from=not-a-date"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn history_api_rejects_limit_edges_and_empty_time_windows() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;
    let writer = actor("writer", RoleMode::Writer);
    let token = mmdash_token("limit-test", None);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();
    let event_time = created.audit_events[0].created_at.replace('+', "%2B");

    for limit in ["0", "501", "-1"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/pages/{page_id}/history?category=all&limit={limit}"
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    let empty = get_json_auth(
        app,
        &format!(
            "/pages/{page_id}/history?category=all&from=2999-01-01T00:00:00Z&to={event_time}"
        ),
        &token,
    )
    .await;
    assert_eq!(empty.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn mcp_history_diff_returns_json_rpc_error_when_version_is_missing() {
    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;
    let writer = actor("writer", RoleMode::Writer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Draft".to_string(),
        make_blocks_json(&["one"]),
    )
    .await
    .unwrap();
    let page_id = created.page.id.clone();
    let created_event_id = created.audit_events[0].id.clone();
    let legacy_event_id = new_id();
    sqlx::query("INSERT INTO audit_events (id, page_id, actor_client_id, actor_nickname, role_mode, event_type, details_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&legacy_event_id)
        .bind(&page_id)
        .bind("legacy")
        .bind("Legacy")
        .bind("writer")
        .bind("page.content_updated")
        .bind("{}")
        .bind(now())
        .execute(&pool)
        .await
        .unwrap();

    let response = mcp_tool(
        app,
        99,
        "history_diff",
        json!({ "page_id": page_id, "from": created_event_id, "to": legacy_event_id }),
    )
    .await;

    assert_eq!(response["id"], 99);
    assert!(response.get("result").is_none());
    assert_eq!(response["error"]["code"], -32000);
    assert_eq!(response["error"]["message"], "version data not available");
}

#[tokio::test]
async fn mcp_initialize_list_and_call_work_over_http() {
    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;

    let initialized = mcp_rpc(
        app.clone(),
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
    )
    .await;
    assert_eq!(initialized["result"]["serverInfo"]["name"], "documosa");

    let listed = mcp_rpc(
        app.clone(),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|tool| tool["name"] == "history_list"));
    assert!(tools.iter().any(|tool| tool["name"] == "history_diff"));
    assert!(tools.iter().all(|tool| {
        tool["inputSchema"]["type"] == "object"
            && tool["inputSchema"]["additionalProperties"] == false
    }));

    let created = mcp_tool(
        app.clone(),
        3,
        "page_create",
        json!({
            "client_id": "mcp",
            "nickname": "MCP",
            "role_mode": "writer",
            "title": "MCP Doc",
            "content": "hello",
            "content_format": "markdown"
        }),
    )
    .await;
    assert_eq!(
        created["result"]["structuredContent"]["page"]["title"],
        "MCP Doc"
    );
    assert!(
        created["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("MCP Doc")
    );
    let page_id = created["result"]["structuredContent"]["page"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let first_block_id = created["result"]["structuredContent"]["blocks"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let created_event_id = created["result"]["structuredContent"]["audit_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["event_type"] == "page.created")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let appended = mcp_tool(
        app.clone(),
        4,
        "block_append",
        json!({
            "client_id": "mcp",
            "nickname": "MCP",
            "role_mode": "writer",
            "page_id": page_id,
            "after": first_block_id,
            "blocks": serde_json::to_string(&[json!({
                "block_type": "paragraph",
                "content_json": rich_text_json("there")
            })]).unwrap(),
        }),
    )
    .await;
    let appended_event_id = appended["result"]["structuredContent"]["audit_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["event_type"] == "blocks.appended")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let history = mcp_tool(
        app.clone(),
        5,
        "history_list",
        json!({ "page_id": page_id, "category": "all" }),
    )
    .await;
    assert!(
        history["result"]["structuredContent"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event_type"] == "blocks.appended")
    );

    let diffed = mcp_tool(
        app.clone(),
        6,
        "history_diff",
        json!({ "page_id": page_id, "from": created_event_id, "to": appended_event_id }),
    )
    .await;
    assert!(
        diffed["result"]["structuredContent"]["unified_diff"]
            .as_str()
            .unwrap()
            .contains("+there")
    );
    assert!(
        diffed["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("@@")
    );
    assert_eq!(
        diffed["result"]["content"][0]["text"],
        diffed["result"]["structuredContent"]["unified_diff"]
    );
    assert_eq!(
        diffed["result"]["structuredContent"]["from_event"]["id"],
        created_event_id
    );
    assert_eq!(
        diffed["result"]["structuredContent"]["to_event"]["id"],
        appended_event_id
    );
    assert_eq!(
        diffed["result"]["structuredContent"]["from_content"],
        "hello"
    );
    assert_eq!(
        diffed["result"]["structuredContent"]["to_content"],
        "hello\nthere"
    );

    let noted = mcp_tool(
        app.clone(),
        7,
        "set_audit_note",
        json!({
            "client_id": "mcp-reviewer",
            "nickname": "MCP Reviewer",
            "page_id": page_id,
            "audit_event_id": appended_event_id,
            "body": "mcp note"
        }),
    )
    .await;
    let noted_event = noted["result"]["structuredContent"]["audit_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["id"] == appended_event_id)
        .unwrap();
    assert_eq!(noted_event["note_body"], "mcp note");

    let cleared = mcp_tool(
        app.clone(),
        8,
        "clear_audit_note",
        json!({
            "client_id": "mcp-reviewer",
            "nickname": "MCP Reviewer",
            "page_id": page_id,
            "audit_event_id": appended_event_id
        }),
    )
    .await;
    let cleared_event = cleared["result"]["structuredContent"]["audit_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["id"] == appended_event_id)
        .unwrap();
    assert_eq!(cleared_event["note_body"], Value::Null);
}

#[tokio::test]
async fn serve_listener_advances_when_port_is_busy() {
    let occupied = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let occupied_port = occupied.local_addr().unwrap().port();

    let listener = documosa::bind_available_listener("127.0.0.1", occupied_port)
        .await
        .unwrap();

    assert_eq!(listener.local_addr().unwrap().port(), occupied_port + 1);
}

#[tokio::test]
async fn cli_notion_document_commands_use_native_api() {
    let (base_url, state) = spawn_notion_mock();

    let create_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "document",
            "create",
            "--notion-token",
            "secret-token",
            "--notion-api-base-url",
            &base_url,
            "--notion-parent-page-id",
            "parent-page",
            "--title",
            "Notion Doc",
            "--content",
            "one\ntwo",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let created: Value = serde_json::from_slice(&create_output).unwrap();
    assert_eq!(created["document"]["id"], "page-1");
    assert_eq!(created["document"]["title"], "Notion Doc");
    assert_eq!(created["lines"].as_array().unwrap().len(), 2);
    assert_eq!(created["lines"][0]["id"], "line-1");
    assert_eq!(created["lines"][1]["content"], "two");
    assert_eq!(created["comments"], json!([]));
    assert_eq!(created["revision"], Value::Null);

    let list_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "document",
            "list",
            "--notion-token",
            "secret-token",
            "--notion-api-base-url",
            &base_url,
            "--notion-parent-page-id",
            "parent-page",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed: Value = serde_json::from_slice(&list_output).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], "page-1");
    assert_eq!(listed[0]["title"], "Listed Doc");

    let export_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "document",
            "export",
            "--notion-token",
            "secret-token",
            "--notion-api-base-url",
            &base_url,
            "page-1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8(export_output).unwrap(), "one\ntwo\n");

    let missing_parent = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "document",
            "create",
            "--notion-token",
            "secret-token",
            "--notion-api-base-url",
            &base_url,
            "--title",
            "Missing Parent",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(
        String::from_utf8(missing_parent)
            .unwrap()
            .contains("--notion-parent-page-id is required in Notion mode")
    );

    let requests = recorded_notion_requests(&state).await;
    assert!(requests.iter().any(|request| {
        request.method == "POST"
            && request.path == "/v1/pages"
            && request.body["parent"]["page_id"] == "parent-page"
            && request.body["properties"]["title"]["title"][0]["text"]["content"] == "Notion Doc"
    }));
    assert!(requests.iter().any(|request| {
        request.method == "PATCH"
            && request.path == "/v1/blocks/page-1/children"
            && request.body["children"].as_array().unwrap().len() == 2
            && request.body["children"][0]["paragraph"]["rich_text"][0]["text"]["content"] == "one"
    }));
    assert!(requests.iter().all(|request| {
        request.authorization.as_deref() == Some("Bearer secret-token")
            && request.notion_version.as_deref() == Some("2026-03-11")
    }));
}

#[tokio::test]
async fn cli_notion_line_commands_validate_and_mutate_paragraph_blocks() {
    let (base_url, state) = spawn_notion_mock();

    Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "line",
            "insert",
            "--notion-token",
            "secret-token",
            "--notion-api-base-url",
            &base_url,
            "page-1",
            "--line",
            "top",
        ])
        .assert()
        .success();

    Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "line",
            "insert",
            "--notion-token",
            "secret-token",
            "--notion-api-base-url",
            &base_url,
            "page-1",
            "--after-line-id",
            "line-1",
            "--line",
            "after one",
        ])
        .assert()
        .success();

    Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "line",
            "replace",
            "--notion-token",
            "secret-token",
            "--notion-api-base-url",
            &base_url,
            "page-1",
            "--line-id",
            "line-1",
            "--line",
            "ONE",
        ])
        .assert()
        .success();

    let invalid_delete = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "line",
            "delete",
            "--notion-token",
            "secret-token",
            "--notion-api-base-url",
            &base_url,
            "page-1",
            "--line-id",
            "not-in-page",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(
        String::from_utf8(invalid_delete)
            .unwrap()
            .contains("line_id not-in-page is not a paragraph block in document page-1")
    );

    let requests = recorded_notion_requests(&state).await;
    assert!(requests.iter().any(|request| {
        request.method == "PATCH"
            && request.path == "/v1/blocks/page-1/children"
            && request.body["position"]["type"] == "start"
            && request.body["children"][0]["paragraph"]["rich_text"][0]["text"]["content"] == "top"
    }));
    assert!(requests.iter().any(|request| {
        request.method == "PATCH"
            && request.path == "/v1/blocks/page-1/children"
            && request.body["position"]["type"] == "after_block"
            && request.body["position"]["after_block"]["id"] == "line-1"
            && request.body["children"][0]["paragraph"]["rich_text"][0]["text"]["content"]
                == "after one"
    }));
    assert!(requests.iter().any(|request| {
        request.method == "PATCH"
            && request.path == "/v1/blocks/line-1"
            && request.body["paragraph"]["rich_text"][0]["text"]["content"] == "ONE"
    }));
    assert!(
        !requests
            .iter()
            .any(|request| request.method == "DELETE" && request.path == "/v1/blocks/not-in-page")
    );
}

#[tokio::test]
async fn cli_notion_rejects_unsupported_command_groups() {
    let output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "history",
            "list",
            "--notion-token",
            "secret-token",
            "page-1",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("Notion backend supports only document and line commands")
    );
}

#[tokio::test]
async fn cli_commands_call_server_api() {
    let temp = TempDir::new().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut server = ProcessCommand::new(assert_cmd::cargo::cargo_bin("documosa"))
        .args([
            "serve",
            "--addr",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--data-dir",
            temp.path().to_str().unwrap(),
        ])
        .env("JWT_SECRET", MMDASH_JWT_SECRET)
        .spawn()
        .unwrap();

    let base = format!("http://127.0.0.1:{port}");
    for _ in 0..50 {
        if reqwest::get(format!("{base}/api/health")).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    ensure_jwt_secret();
    let token = mmdash_token("cli-user", None);

    // Create page
    let create_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "document",
            "create",
            "--server",
            &base,
            "--jwt",
            &token,
            "--role-mode",
            "writer",
            "--title",
            "CLI",
            "--content",
            "",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let created: Value = serde_json::from_slice(&create_output).unwrap();
    let page_id = created["page"]["id"].as_str().unwrap().to_string();

    // List documents
    let list_output = Command::cargo_bin("documosa")
        .unwrap()
        .args(["document", "list", "--server", &base, "--jwt", &token])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(list_output).unwrap();
    assert!(text.contains("CLI"));

    // Insert a line (creates a block)
    let insert_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "line",
            "insert",
            "--server",
            &base,
            "--jwt",
            &token,
            "--role-mode",
            "writer",
            &page_id,
            "--line",
            "second cli line",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inserted: Value = serde_json::from_slice(&insert_output).unwrap();
    let first_block_id = inserted["blocks"][0]["id"].as_str().unwrap().to_string();

    // Create suggestion
    Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "suggestion",
            "create",
            "--server",
            &base,
            "--jwt",
            &token,
            &page_id,
            "--kind",
            "replace",
            "--target-block-id",
            &first_block_id,
            "--line",
            "suggested replacement",
        ])
        .assert()
        .success();

    // List all history
    let history_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "history",
            "list",
            "--server",
            &base,
            "--jwt",
            &token,
            &page_id,
            "--category",
            "all",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let history: Value = serde_json::from_slice(&history_output).unwrap();
    let history_events = history.as_array().unwrap();
    let created_event_id = history_events
        .iter()
        .find(|event| event["event_type"] == "page.created")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let inserted_event_id = history_events
        .iter()
        .find(|event| event["event_type"] == "blocks.appended")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        history_events
            .iter()
            .any(|event| event["event_type"] == "suggestion.created")
    );

    // Suggestion-only history
    let suggestion_history_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "history",
            "list",
            "--server",
            &base,
            "--jwt",
            &token,
            &page_id,
            "--category",
            "suggestion",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let suggestion_history: Value = serde_json::from_slice(&suggestion_history_output).unwrap();
    assert_eq!(
        suggestion_history.as_array().unwrap()[0]["event_type"],
        "suggestion.created"
    );

    // Diff
    let diff_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "history",
            "diff",
            "--server",
            &base,
            "--jwt",
            &token,
            &page_id,
            "--from",
            &created_event_id,
            "--to",
            &inserted_event_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let diff_text = String::from_utf8(diff_output).unwrap();
    assert!(diff_text.contains("--- "));
    assert!(diff_text.contains("+++ "));
    assert!(diff_text.contains("@@"));
    assert!(diff_text.contains("+second cli line"));

    // Zero context diff
    let zero_context_diff_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "history",
            "diff",
            "--server",
            &base,
            "--jwt",
            &token,
            &page_id,
            "--from",
            &created_event_id,
            "--to",
            &inserted_event_id,
            "--context",
            "0",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let zero_context_diff_text = String::from_utf8(zero_context_diff_output).unwrap();
    assert!(zero_context_diff_text.contains("@@"));
    assert!(zero_context_diff_text.contains("+second cli line"));

    // Missing diff
    let missing_diff = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "history",
            "diff",
            "--server",
            &base,
            "--jwt",
            &token,
            &page_id,
            "--from",
            &inserted_event_id,
            "--to",
            "missing-audit-event",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let missing_diff_stderr = String::from_utf8(missing_diff).unwrap();
    assert!(missing_diff_stderr.contains("404"));

    // Set note
    let noted_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "history",
            "note",
            "set",
            "--server",
            &base,
            "--jwt",
            &token,
            &page_id,
            &inserted_event_id,
            "--body",
            "cli note",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let noted: Value = serde_json::from_slice(&noted_output).unwrap();
    assert!(
        noted["audit_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["id"] == inserted_event_id && event["note_body"] == "cli note")
    );

    // Clear note
    let cleared_output = Command::cargo_bin("documosa")
        .unwrap()
        .args([
            "history",
            "note",
            "clear",
            "--server",
            &base,
            "--jwt",
            &token,
            &page_id,
            &inserted_event_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cleared: Value = serde_json::from_slice(&cleared_output).unwrap();
    assert!(
        cleared["audit_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["id"] == inserted_event_id && event["note_body"] == Value::Null)
    );

    let _ = server.kill();
    let _ = server.wait();
}

#[tokio::test]
async fn static_file_serve_rejects_path_traversal() {
    let pool = pool().await;
    let temp = TempDir::new().unwrap();
    let web_dir = temp.path().join("web");
    std::fs::create_dir_all(&web_dir).unwrap();
    std::fs::write(web_dir.join("index.html"), "<html></html>").unwrap();
    std::fs::write(web_dir.join("secret.txt"), "secret").unwrap();
    let parent_dir = temp.path().join("outside");
    std::fs::create_dir_all(&parent_dir).unwrap();
    std::fs::write(parent_dir.join("sensitive.txt"), "sensitive").unwrap();

    let app = documosa::build_app(pool, web_dir.clone()).await;

    // Direct parent-dir escape attempt
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/../outside/sensitive.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Nested parent-dir escape attempt
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/secret.txt/../../outside/sensitive.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Valid file within web_dir should succeed
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/secret.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(String::from_utf8(bytes.to_vec()).unwrap(), "secret");

    // Valid nested path with harmless .. segments should succeed
    let nested = web_dir.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("file.txt"), "nested").unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/nested/../secret.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(String::from_utf8(bytes.to_vec()).unwrap(), "secret");
}

#[tokio::test]
async fn export_page_returns_not_found_for_unknown_id() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;
    let token = mmdash_token("test-user", None);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/pages/not-a-real-id/export/md")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn page_creation_with_multiple_blocks() {
    let pool = pool().await;
    let writer = actor("writer", RoleMode::Writer);

    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Multi-block".to_string(),
        make_blocks_json(&["# Title", "Paragraph text.", "> Quote"]),
    )
    .await
    .unwrap();

    assert_eq!(created.blocks.len(), 3);
    assert_eq!(created.blocks[0].block_type, "paragraph");
    assert_eq!(block_text(&created.blocks[0]), "# Title");
    assert_eq!(block_text(&created.blocks[2]), "> Quote");

    let markdown = documosa::db::export_markdown(&pool, &created.page.id)
        .await
        .unwrap();
    assert!(markdown.contains("Multi-block"));
}

// ─── mmdash adapter integration tests ───────────────────────────────

#[tokio::test]
async fn mmdash_create_document_with_valid_jwt() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;

    let token = mmdash_token("user-1", Some("Alice"));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/mmdash/documents")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(
                    json!({"title": "New Doc", "content": "# Hello\n\nWorld"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["title"], "New Doc");
    assert!(!body["page_id"].as_str().unwrap().is_empty());
    assert!(body["created_at"].as_str().unwrap().contains('T'));
}

#[tokio::test]
async fn mmdash_rejects_invalid_jwt() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;

    let bad_token = "invalid.token.here";
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/mmdash/documents")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {bad_token}"))
                .body(Body::from(json!({"title": "X"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "unauthorized");
}

#[tokio::test]
async fn mmdash_rejects_missing_jwt() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/mmdash/documents")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"title": "X"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mmdash_get_content_returns_blocks_and_markdown() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;

    let writer = actor("writer", RoleMode::Writer);
    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Blocks Doc".to_string(),
        make_blocks_json(&["# Title", "Paragraph.", "Bullet", "Numbered", "Quote", "---"]),
    )
    .await
    .unwrap();
    let document_id = created.page.id;

    let token = mmdash_token("user-3", None);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/mmdash/documents/{document_id}/content"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["page_id"], document_id);
    assert_eq!(body["title"], "Blocks Doc");
    assert!(body["markdown"].as_str().unwrap().contains("# Title"));
    let blocks = body["blocks"].as_array().unwrap();
    assert_eq!(blocks[0]["type"], "paragraph");
    assert_eq!(blocks[0]["content"], "# Title");
    assert_eq!(blocks[1]["type"], "paragraph");
    assert_eq!(blocks[2]["type"], "paragraph");
    // Block types are all "paragraph" since make_blocks_json always uses paragraph type
    // The mmdash conversion mainly handles block type mapping from snapshot blocks
}

#[tokio::test]
async fn mmdash_get_document_metadata() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;

    let writer = actor("writer", RoleMode::Writer);
    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Meta Doc".to_string(),
        make_blocks_json(&["content"]),
    )
    .await
    .unwrap();
    let document_id = created.page.id;

    let token = mmdash_token("user-4", None);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/mmdash/documents/{document_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["page_id"], document_id);
    assert_eq!(body["title"], "Meta Doc");
    assert!(body["created_at"].as_str().unwrap().contains('T'));
    assert!(body["updated_at"].as_str().unwrap().contains('T'));
}

#[tokio::test]
async fn mmdash_returns_404_for_missing_document() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;

    let token = mmdash_token("user-5", None);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/mmdash/documents/non-existent-id/content")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mmdash_put_content_updates_document() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;

    let writer = actor("writer", RoleMode::Writer);
    let created = documosa::db::create_page(
        &pool,
        &writer,
        "Update Doc".to_string(),
        make_blocks_json(&["old content"]),
    )
    .await
    .unwrap();
    let document_id = created.page.id;

    let token = mmdash_token("user-7", None);
    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/mmdash/documents/{document_id}/content"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(
                    json!({
                        "title": "Updated Title",
                        "blocks": [
                            {"type": "heading_1", "content": "New Heading"},
                            {"type": "paragraph", "content": "New paragraph."}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["title"], "Updated Title");
    assert_eq!(body["blocks"][0]["type"], "heading_1");
    assert_eq!(body["blocks"][0]["content"], "New Heading");
    assert_eq!(body["blocks"][1]["type"], "paragraph");
    assert_eq!(body["blocks"][1]["content"], "New paragraph.");
    assert!(body["markdown"].as_str().unwrap().contains("# New Heading"));
}

#[tokio::test]
async fn mmdash_identity_derived_from_token() {
    ensure_jwt_secret();

    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;

    let token = mmdash_token("user-abc", Some("Bob"));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/mmdash/documents")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(json!({"title": "Identity Test"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let page_id = body["page_id"].as_str().unwrap();

    let snapshot = documosa::db::snapshot(&pool, page_id).await.unwrap();
    let audit_event = snapshot
        .audit_events
        .iter()
        .find(|event| event.event_type == "page.created")
        .unwrap();
    assert_eq!(audit_event.actor_client_id, "mmdash-user-abc");
    assert_eq!(audit_event.actor_nickname, "Bob");
    assert_eq!(audit_event.role_mode, "writer");
}

// ─── Helper for history API calls ───

async fn get_json_auth(app: axum::Router, uri: &str, token: &str) -> Value {
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
