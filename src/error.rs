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
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Protocol(ProtocolError::BadRequest(_)) => StatusCode::BAD_REQUEST,
            AppError::Protocol(ProtocolError::Forbidden(_)) => StatusCode::FORBIDDEN,
            AppError::Protocol(ProtocolError::Conflict(_)) => StatusCode::CONFLICT,
            AppError::Protocol(ProtocolError::NotFound) => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Sqlx(sqlx::Error::RowNotFound) => StatusCode::NOT_FOUND,
            AppError::Sqlx(_) | AppError::Io(_) | AppError::Json(_) | AppError::Anyhow(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        let error = self.to_string();
        let message = match &self {
            AppError::Protocol(ProtocolError::BadRequest(msg))
            | AppError::Protocol(ProtocolError::Forbidden(msg))
            | AppError::Protocol(ProtocolError::Conflict(msg)) => Some(msg.clone()),
            AppError::BadRequest(msg) | AppError::Forbidden(msg) | AppError::Conflict(msg) => {
                Some(msg.clone())
            }
            AppError::Unauthorized => Some("invalid token".into()),
            _ => None,
        };
        let body = Json(ErrorBody { error, message });
        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
