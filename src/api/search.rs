use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::Result;
use crate::mmdash_auth::MmdashIdentity;
use crate::AppState;

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
    value: Option<String>,
}

fn page_to_result(p: &crate::models::Page) -> Value {
    let title_tokens: Value = serde_json::from_str(&p.title_json).unwrap_or(json!([]));
    json!({
        "object": "page",
        "id": p.id,
        "created_time": p.created_at,
        "last_edited_time": p.updated_at,
        "properties": {
            "title": { "id": "title", "type": "title", "title": title_tokens }
        }
    })
}

async fn search(
    State(state): State<AppState>,
    MmdashIdentity(_identity): MmdashIdentity,
    Json(body): Json<SearchRequest>,
) -> Result<impl IntoResponse> {
    let filter_value = body.filter.as_ref()
        .and_then(|f| f.value.as_deref()).unwrap_or("page");
    let query = format!("%{}%", body.query);
    let mut map = serde_json::Map::new();
    map.insert("object".into(), json!("list"));

    if filter_value == "block" {
        let blocks: Vec<crate::models::Block> = sqlx::query_as(
            "SELECT id, page_id, parent_id, order_index, block_type, content_json, properties_json, revision, deleted, created_at, updated_at FROM blocks WHERE deleted = 0 AND content_json LIKE ? ORDER BY updated_at DESC LIMIT 50"
        ).bind(&query).fetch_all(&state.pool).await.map_err(crate::error::AppError::from)?;
        let results: Vec<Value> = blocks.iter().map(|b| json!({
            "object": "block", "id": b.id, "type": b.block_type,
            "page_id": b.page_id, "content_json": b.content_json,
            "created_time": b.created_at, "last_edited_time": b.updated_at,
        })).collect();
        map.insert("results".into(), json!(results));
    } else {
        let pages: Vec<crate::models::Page> = sqlx::query_as(
            "SELECT id, title_json, properties_json, created_at, updated_at FROM pages WHERE title_json LIKE ? ORDER BY updated_at DESC LIMIT 50"
        ).bind(&query).fetch_all(&state.pool).await.map_err(crate::error::AppError::from)?;
        let results: Vec<Value> = pages.iter().map(page_to_result).collect();
        map.insert("results".into(), json!(results));
    }
    map.insert("next_cursor".into(), Value::Null);
    map.insert("has_more".into(), json!(false));
    Ok(Json(Value::Object(map)))
}
