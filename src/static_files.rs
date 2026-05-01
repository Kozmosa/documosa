use std::path::Path;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::AppState;

pub async fn serve(State(state): State<AppState>, request: Request<Body>) -> Response {
    let path = request.uri().path().trim_start_matches('/');

    // Security: prevent path traversal by tracking segment depth
    let mut depth = 0i32;
    for segment in path.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return StatusCode::FORBIDDEN.into_response();
                }
            }
            _ => depth += 1,
        }
    }

    let mut candidate = if path.is_empty() {
        state.web_dir.join("index.html")
    } else {
        state.web_dir.join(path)
    };
    if candidate.is_dir() {
        candidate = candidate.join("index.html");
    }
    if !candidate.exists() {
        candidate = state.web_dir.join("index.html");
    }

    // Defense in depth: verify resolved path stays within web_dir
    if let (Ok(web_dir), Ok(resolved)) = (
        std::fs::canonicalize(&*state.web_dir),
        std::fs::canonicalize(&candidate),
    ) && !resolved.starts_with(&web_dir)
    {
        return StatusCode::FORBIDDEN.into_response();
    }

    match tokio::fs::read(&candidate).await {
        Ok(bytes) => {
            let content_type = content_type(&candidate);
            let mut response = Response::new(Body::from(bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            response
        }
        Err(_) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<div id=\"root\"></div><script>document.body.innerHTML='Build the web app with npm run build or use the API directly.'</script>",
        )
            .into_response(),
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or("") {
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "text/html; charset=utf-8",
    }
}
