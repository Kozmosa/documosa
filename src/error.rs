use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use documosa_core::error::ProtocolError;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Protocol(#[from] ProtocolError),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    Conflict(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    object: String,
    status: u16,
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AppError::Protocol(ProtocolError::BadRequest(msg)) => {
                (StatusCode::BAD_REQUEST, "validation_error", Some(msg.clone()))
            }
            AppError::Protocol(ProtocolError::Forbidden(msg)) => {
                (StatusCode::FORBIDDEN, "restricted_resource", Some(msg.clone()))
            }
            AppError::Protocol(ProtocolError::Conflict(msg)) => {
                (StatusCode::CONFLICT, "conflict_error", Some(msg.clone()))
            }
            AppError::Protocol(ProtocolError::NotFound) => {
                (StatusCode::NOT_FOUND, "object_not_found", None)
            }
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "validation_error", Some(msg.clone())),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, "restricted_resource", Some(msg.clone())),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, "conflict_error", Some(msg.clone())),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", None),
            AppError::NotFound => (StatusCode::NOT_FOUND, "object_not_found", None),
            AppError::Sqlx(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "object_not_found", None),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error", None),
        };
        let body = Json(ErrorBody {
            object: "error".into(),
            status: status.as_u16(),
            code: code.into(),
            message,
        });
        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
