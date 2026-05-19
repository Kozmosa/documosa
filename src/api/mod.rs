mod pages;
mod blocks;
mod comments;
mod suggestions;
mod history;
mod export;
mod ws;

use axum::Router;
use serde::Serialize;
use serde_json::{Value, json, Map};

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
        .merge(ws::router());
    Router::new()
        .nest("/v1", v1)
        .merge(mmdash_api::router())
}
