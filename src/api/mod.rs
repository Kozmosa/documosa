mod pages;
mod blocks;
mod comments;
mod suggestions;
mod history;
mod export;
mod ws;

use axum::Router;
use crate::AppState;
use crate::mmdash_api;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(pages::router())
        .merge(blocks::router())
        .merge(comments::router())
        .merge(suggestions::router())
        .merge(history::router())
        .merge(export::router())
        .merge(ws::router())
        .merge(mmdash_api::router())
}
