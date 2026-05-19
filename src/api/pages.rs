use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::{wrap_list, wrap_object};
use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::models::Page;
use crate::mmdash_auth::MmdashIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pages", post(create_page).get(list_pages))
        .route("/pages/{page_id}", get(get_page).patch(update_page))
        .route("/pages/{page_id}/snapshot", get(get_snapshot))
}

#[derive(Deserialize)]
struct UpdatePageRequest {
    title: Option<String>,          // legacy
    properties: Option<Value>,      // Notion format
}

fn page_to_notion_response(page: &Page) -> Value {
    let title_tokens: Value = serde_json::from_str(&page.title_json).unwrap_or(json!([]));
    json!({
        "object": "page",
        "id": page.id,
        "created_time": page.created_at,
        "last_edited_time": page.updated_at,
        "properties": {
            "title": {
                "id": "title",
                "type": "title",
                "title": title_tokens,
            }
        },
        "properties_json": page.properties_json,
    })
}

async fn create_page(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse> {
    let title_rich_text = body
        .get("properties")
        .and_then(|p| p.get("title"))
        .and_then(|t| t.get("title"))
        .cloned()
        .unwrap_or_else(|| {
            let title_str = body.get("title").and_then(Value::as_str).unwrap_or("");
            json!([{"type":"text","text":{"content":title_str},"plain_text":title_str}])
        });

    let title_json = serde_json::to_string(&title_rich_text).unwrap_or_default();
    let snap = db::create_page(&state.pool, &actor, title_json, String::new()).await?;
    state.hub.page_created(&snap.page.id);
    Ok((StatusCode::CREATED, Json(page_to_notion_response(&snap.page))))
}

async fn list_pages(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
) -> Result<impl IntoResponse> {
    let pages = db::list_pages(&state.pool).await?;
    let results: Vec<Value> = pages.iter().map(|p| page_to_notion_response(p)).collect();
    Ok(Json(wrap_list(serde_json::to_value(results).unwrap())))
}

async fn get_page(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(page_id): Path<String>,
) -> Result<impl IntoResponse> {
    let page = db::get_page(&state.pool, &page_id).await?;
    Ok(Json(page_to_notion_response(&page)))
}

async fn update_page(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path(page_id): Path<String>,
    Json(body): Json<UpdatePageRequest>,
) -> Result<impl IntoResponse> {
    if let Some(ref title) = body.title {
        let rt = json!([{"type":"text","text":{"content":title},"plain_text":title}]);
        let title_json = serde_json::to_string(&rt).unwrap_or_default();
        db::update_page_title(&state.pool, &actor, &page_id, &title_json).await?;
        state.hub.title_updated(&page_id, title);
    } else if let Some(ref properties) = body.properties {
        if let Some(title_prop) = properties.get("title") {
            if let Some(title_tokens) = title_prop.get("title") {
                let title_json = serde_json::to_string(title_tokens).unwrap_or_default();
                let plain = Page::plain_title_from_json(&title_json);
                db::update_page_title(&state.pool, &actor, &page_id, &title_json).await?;
                state.hub.title_updated(&page_id, &plain);
            }
        }
    }
    let page = db::get_page(&state.pool, &page_id).await?;
    Ok(Json(page_to_notion_response(&page)))
}

async fn get_snapshot(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(page_id): Path<String>,
) -> Result<impl IntoResponse> {
    let snap = db::snapshot(&state.pool, &page_id).await?;
    let mut val = serde_json::to_value(&snap).unwrap();
    if let Some(obj) = val.as_object_mut() {
        let page_val = page_to_notion_response(&snap.page);
        obj.insert("page".into(), page_val);
    }
    Ok(Json(wrap_object("page", val)))
}
