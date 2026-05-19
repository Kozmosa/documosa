# Documosa Users + Search Endpoints — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `GET /v1/users` + `GET /v1/users/{id}` (mock) and `POST /v1/search` (SQL LIKE query) to reach full Notion endpoint coverage.

**Architecture:** Two new API module files. Users returns mock data from JWT claims. Search queries pages and blocks via SQL LIKE on plain_text content. No new database tables.

**Tech Stack:** Rust 2024, axum, sqlx, serde_json.

---

### Task 1: Users mock endpoint

**Files:**
- Create: `src/api/users.rs`
- Modify: `src/api/mod.rs`

- [ ] **Step 1: Create `src/api/users.rs`**

```rust
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::AppState;
use crate::error::Result;
use crate::mmdash_auth::MmdashIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users))
        .route("/users/{user_id}", get(get_user))
}

fn current_user_id(identity: &MmdashIdentity) -> String {
    format!("user-{}", identity.0.client_id.trim_start_matches("mmdash-"))
}

async fn list_users(
    State(_state): State<AppState>,
    MmdashIdentity(identity): MmdashIdentity,
) -> Result<impl IntoResponse> {
    let current_user = json!({
        "object": "user",
        "id": current_user_id(&MmdashIdentity(identity.clone())),
        "type": "person",
        "name": identity.nickname,
        "avatar_url": null,
        "person": { "email": format!("{}@documosa.local", identity.client_id) },
    });
    let bot_user = json!({
        "object": "user",
        "id": "documosa-agent",
        "type": "bot",
        "name": "Documosa Agent",
        "avatar_url": null,
        "bot": {},
    });
    let mut map = serde_json::Map::new();
    map.insert("object".into(), json!("list"));
    map.insert("results".into(), json!([current_user, bot_user]));
    map.insert("next_cursor".into(), Value::Null);
    map.insert("has_more".into(), json!(false));
    Ok(Json(Value::Object(map)))
}

async fn get_user(
    State(_state): State<AppState>,
    MmdashIdentity(identity): MmdashIdentity,
    Path(user_id): Path<String>,
) -> Result<impl IntoResponse> {
    let current_id = current_user_id(&MmdashIdentity(identity.clone()));
    if user_id == current_id {
        return Ok(Json(json!({
            "object": "user",
            "id": current_id,
            "type": "person",
            "name": identity.nickname,
            "avatar_url": null,
            "person": { "email": format!("{}@documosa.local", identity.client_id) },
        })));
    }
    if user_id == "documosa-agent" {
        return Ok(Json(json!({
            "object": "user",
            "id": "documosa-agent",
            "type": "bot",
            "name": "Documosa Agent",
            "avatar_url": null,
            "bot": {},
        })));
    }
    Err(crate::error::AppError::NotFound)
}
```

- [ ] **Step 2: Add `mod users;` and merge router in `src/api/mod.rs`**

In the v1 router assembly, add:
```rust
.use(users::router())
```

- [ ] **Step 3: Run tests and commit**

```bash
cargo test 2>&1
git add src/api/users.rs src/api/mod.rs
git commit -m "feat: add users mock endpoint (GET /v1/users, GET /v1/users/{id})"
```

---

### Task 2: Search endpoint

**Files:**
- Create: `src/api/search.rs`
- Modify: `src/api/mod.rs`

- [ ] **Step 1: Create `src/api/search.rs`**

```rust
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::mmdash_auth::MmdashIdentity;
use crate::models::Page;

pub fn router() -> Router<AppState> {
    Router::new().route("/search", post(search))
}

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    #[serde(default)]
    filter: Option<SearchFilter>,
}

#[derive(Deserialize)]
struct SearchFilter {
    property: Option<String>,
    value: Option<String>,
}

async fn search(
    State(state): State<AppState>,
    MmdashIdentity(_identity): MmdashIdentity,
    Json(body): Json<SearchRequest>,
) -> Result<impl IntoResponse> {
    let search_type = body.filter.as_ref()
        .and_then(|f| f.property.as_deref())
        .unwrap_or("");
    let filter_value = body.filter.as_ref()
        .and_then(|f| f.value.as_deref())
        .unwrap_or("page");
    
    let query = format!("%{}%", body.query);
    let mut map = serde_json::Map::new();
    map.insert("object".into(), json!("list"));

    match (search_type, filter_value) {
        (_, "page") | ("object", "page") => {
            let pages: Vec<Page> = sqlx::query_as(
                "SELECT id, title_json, properties_json, created_at, updated_at FROM pages WHERE title_json LIKE ? ORDER BY updated_at DESC LIMIT 50"
            )
            .bind(&query)
            .fetch_all(&state.pool)
            .await
            .map_err(crate::error::AppError::from)?;
            
            let results: Vec<Value> = pages.iter().map(|p| {
                let title_tokens: Value = serde_json::from_str(&p.title_json).unwrap_or(json!([]));
                json!({
                    "object": "page",
                    "id": p.id,
                    "created_time": p.created_at,
                    "last_edited_time": p.updated_at,
                    "properties": {
                        "title": {
                            "id": "title",
                            "type": "title",
                            "title": title_tokens,
                        }
                    }
                })
            }).collect();
            map.insert("results".into(), json!(results));
        }
        (_, "block") | ("object", "block") => {
            let blocks: Vec<crate::models::Block> = sqlx::query_as(
                "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE deleted = 0 AND content_json LIKE ? ORDER BY updated_at DESC LIMIT 50"
            )
            .bind(&query)
            .fetch_all(&state.pool)
            .await
            .map_err(crate::error::AppError::from)?;
            
            let results: Vec<Value> = blocks.iter().map(|b| {
                json!({
                    "object": "block",
                    "id": b.id,
                    "type": b.block_type,
                    "page_id": b.page_id,
                    "content_json": b.content_json,
                    "created_time": b.created_at,
                    "last_edited_time": b.updated_at,
                })
            }).collect();
            map.insert("results".into(), json!(results));
        }
        _ => {
            map.insert("results".into(), json!([]));
        }
    }
    map.insert("next_cursor".into(), Value::Null);
    map.insert("has_more".into(), json!(false));
    Ok(Json(Value::Object(map)))
}
```

- [ ] **Step 2: Add `mod search;` and merge router in `src/api/mod.rs`**

- [ ] **Step 3: Run tests and commit**

```bash
cargo test 2>&1
git add src/api/search.rs src/api/mod.rs
git commit -m "feat: add search endpoint (POST /v1/search)"
```

---

### Task 3: Add tests

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add users test**

```rust
#[tokio::test]
async fn users_list_and_get_work() {
    ensure_jwt_secret();
    let pool = pool().await;
    let app = documosa::build_app(pool, PathBuf::from("missing")).await;
    let token = mmdash_token("test-user", Some("Alice"));

    // List
    let response = app.clone().oneshot(
        Request::builder().method("GET").uri("/v1/users")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["object"], "list");
    assert_eq!(body["results"].as_array().unwrap().len(), 2);
    assert_eq!(body["results"][0]["object"], "user");
    assert_eq!(body["next_cursor"], Value::Null);

    // Get
    let user_id = body["results"][0]["id"].as_str().unwrap();
    let response = app.clone().oneshot(
        Request::builder().method("GET").uri(&format!("/v1/users/{user_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    // Unknown user
    let response = app.oneshot(
        Request::builder().method("GET").uri("/v1/users/nonexistent")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Add search test**

```rust
#[tokio::test]
async fn search_finds_pages_and_blocks() {
    ensure_jwt_secret();
    let pool = pool().await;
    let app = documosa::build_app(pool.clone(), PathBuf::from("missing")).await;
    let token = mmdash_token("test-user", Some("Searcher"));
    let writer = actor("writer", RoleMode::Writer);
    
    let rt = rich_text_json("UniqueKeyword42");
    let snap = documosa::db::create_page(&pool, &writer, rt, String::new()).await.unwrap();
    let page_id = snap.page.id;
    
    // Search pages
    let response = app.clone().oneshot(
        Request::builder().method("POST").uri("/v1/search")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(json!({"query":"UniqueKeyword42","filter":{"property":"object","value":"page"}}).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["object"], "list");
    let results = body["results"].as_array().unwrap();
    assert!(results.iter().any(|r| r["id"] == page_id), "should find matching page");
    
    // Block search (append a block with unique content first)
    let bid = documosa::db::append_blocks(&pool, &writer, &page_id, vec![block_input("SearchTarget99")], None).await.unwrap()[0].id.clone();
    let response = app.oneshot(
        Request::builder().method("POST").uri("/v1/search")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(json!({"query":"SearchTarget99","filter":{"property":"object","value":"block"}}).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["results"].as_array().unwrap().iter().any(|r| r["id"] == bid), "should find matching block");
}
```

- [ ] **Step 3: Run tests and commit**

```bash
cargo test 2>&1
git add tests/integration.rs
git commit -m "test: add integration tests for users and search endpoints"
```

---

### Task 4: Final verification

- [ ] **Step 1: Full test suite + builds**

```bash
cargo test 2>&1 | grep "test result"
cargo build 2>&1 | tail -2
cd web && npm run build 2>&1 | tail -3
```

- [ ] **Step 2: Quick E2E smoke**

```bash
JWT_SECRET=test cargo run -- serve --port 14360 --data-dir /tmp/doc-usearch &
TOKEN=$(python3 -c "import jwt,time; print(jwt.encode({'sub':'u','name':'T','exp':int(time.time())+3600}, 'test', algorithm='HS256'))")

# Users
curl -s http://127.0.0.1:14360/v1/users -H "Authorization: Bearer $TOKEN" | python3 -c "import sys,json; d=json.load(sys.stdin); assert d['object']=='list'; print(f'Users: {len(d[\"results\"])} results')"

# Search pages
PAGE=$(curl -s -X POST http://127.0.0.1:14360/v1/pages -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" -d '{"title":"Searchable"}')
curl -s -X POST http://127.0.0.1:14360/v1/search -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" -d '{"query":"Search"}' | python3 -c "import sys,json; d=json.load(sys.stdin); assert len(d['results'])>0; print(f'Search: {len(d[\"results\"])} pages found')"

kill %1
```

- [ ] **Step 3: Commit any fixes**

```bash
git add -A && git commit -m "chore: final verification for users and search"
```
