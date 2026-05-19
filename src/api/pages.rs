use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::api::{wrap_list, wrap_object};
use crate::AppState;
use crate::db;
use crate::error::Result;
use crate::mmdash_auth::MmdashIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pages", post(create_page).get(list_pages))
        .route("/pages/{page_id}", get(get_page).patch(update_page))
        .route("/pages/{page_id}/snapshot", get(get_snapshot))
}

#[derive(Deserialize)]
struct CreatePageRequest {
    title: String,
    #[serde(default)]
    content_json: String,
}

#[derive(Deserialize)]
struct UpdatePageRequest {
    title: Option<String>,
}

async fn create_page(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Json(body): Json<CreatePageRequest>,
) -> Result<impl IntoResponse> {
    let snap = db::create_page(&state.pool, &actor, body.title, body.content_json).await?;
    state.hub.page_created(&snap.page.id);
    Ok((StatusCode::CREATED, Json(wrap_object("page", snap))))
}

async fn list_pages(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
) -> Result<impl IntoResponse> {
    let pages = db::list_pages(&state.pool).await?;
    Ok(Json(wrap_list(serde_json::to_value(pages).unwrap())))
}

async fn get_page(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(page_id): Path<String>,
) -> Result<impl IntoResponse> {
    let page = db::get_page(&state.pool, &page_id).await?;
    Ok(Json(wrap_object("page", page)))
}

async fn update_page(
    State(state): State<AppState>,
    MmdashIdentity(actor): MmdashIdentity,
    Path(page_id): Path<String>,
    Json(body): Json<UpdatePageRequest>,
) -> Result<impl IntoResponse> {
    if let Some(title) = &body.title {
        db::update_page_title(&state.pool, &actor, &page_id, title).await?;
        state.hub.title_updated(&page_id, title);
    }
    let page = db::get_page(&state.pool, &page_id).await?;
    Ok(Json(wrap_object("page", page)))
}

async fn get_snapshot(
    State(state): State<AppState>,
    MmdashIdentity(_actor): MmdashIdentity,
    Path(page_id): Path<String>,
) -> Result<impl IntoResponse> {
    let snapshot = db::snapshot(&state.pool, &page_id).await?;
    Ok(Json(wrap_object("page", snapshot)))
}
