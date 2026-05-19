use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use crate::AppState;
use crate::error::{AppError, Result};
use crate::models::{Identity, RoleMode};
use crate::realtime;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pages/{page_id}/ws", get(ws_handler))
}

#[derive(Deserialize)]
struct WsQuery {
    client_id: String,
    nickname: String,
    role_mode: String,
}

async fn ws_handler(
    State(state): State<AppState>,
    Path(page_id): Path<String>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse> {
    let identity = Identity {
        client_id: query.client_id,
        nickname: query.nickname,
        role_mode: RoleMode::parse(&query.role_mode)?,
        actor_kind: Default::default(),
    };
    if identity.client_id.trim().is_empty() || identity.nickname.trim().is_empty() {
        return Err(AppError::BadRequest(
            "client id and nickname are required".into(),
        ));
    }
    Ok(ws.on_upgrade(move |socket| realtime::websocket(socket, state.hub, page_id, identity)))
}
