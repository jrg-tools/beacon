use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// An error a handler returns: a deliberate status, or anything else as a logged 500.
pub enum AppError {
    Status(StatusCode, &'static str),
    Internal(anyhow::Error),
}

pub type Result<T> = std::result::Result<T, AppError>;

pub fn status(code: StatusCode, message: &'static str) -> AppError {
    AppError::Status(code, message)
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError::Internal(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (code, message) = match self {
            AppError::Status(code, message) => (code, message),
            AppError::Internal(e) => {
                tracing::error!("{e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
        };
        (code, Json(json!({ "error": message }))).into_response()
    }
}
