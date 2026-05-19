use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::Result;
use crate::mmdash_auth::MmdashIdentity;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users))
        .route("/users/{user_id}", get(get_user))
}

fn current_user_id(identity: &crate::models::Identity) -> String {
    format!("user-{}", identity.client_id.trim_start_matches("mmdash-"))
}

async fn list_users(
    State(_state): State<AppState>,
    MmdashIdentity(identity): MmdashIdentity,
) -> Result<impl IntoResponse> {
    let current_user = json!({
        "object": "user",
        "id": current_user_id(&identity),
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
    let current_id = current_user_id(&identity);
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
