mod pages;
mod blocks;
mod comments;
mod suggestions;
mod history;
mod export;
mod ws;
mod users;
mod search;

use axum::Router;
use axum::http::{HeaderName, HeaderValue};
use serde::Serialize;
use serde_json::{Value, json, Map};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::AppState;
use crate::mmdash_api;

pub(crate) fn wrap_object(object_type: &str, value: impl Serialize) -> Value {
    let mut v = serde_json::to_value(value).unwrap_or(Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.insert("object".into(), json!(object_type));
    }
    v
}

pub(crate) fn wrap_list(value: Value) -> Value {
    let mut map = Map::new();
    map.insert("object".into(), json!("list"));
    map.insert("results".into(), value);
    Value::Object(map)
}

pub fn router() -> Router<AppState> {
    let v1 = Router::new()
        .merge(pages::router())
        .merge(blocks::router())
        .merge(comments::router())
        .merge(suggestions::router())
        .merge(history::router())
        .merge(export::router())
        .merge(ws::router())
        .merge(users::router())
        .merge(search::router())
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("notion-version"),
            HeaderValue::from_static("2022-06-28"),
        ));
    Router::new()
        .nest("/v1", v1)
        .merge(mmdash_api::router())
}
