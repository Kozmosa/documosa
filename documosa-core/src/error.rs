#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    Conflict(String),
    #[error("not found")]
    NotFound,
}

pub type Result<T> = std::result::Result<T, ProtocolError>;
