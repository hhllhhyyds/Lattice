//! Application-level error types for the HTTP server.

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

/// Unified error response body format.
#[derive(Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

/// Application errors that map to HTTP responses.
pub enum AppError {
    /// Session was not found.
    SessionNotFound(lattice_core::SessionId),
    /// Request was malformed or invalid.
    InvalidRequest(String),
    /// Internal server error.
    InternalError(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionNotFound(id) => write!(f, "Session with id {id} does not exist"),
            Self::InvalidRequest(msg) => write!(f, "{msg}"),
            Self::InternalError(msg) => write!(f, "{msg}"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self {
            Self::SessionNotFound(id) => (
                StatusCode::NOT_FOUND,
                "session_not_found",
                format!("Session with id {id} does not exist"),
            ),
            Self::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, "invalid_request", msg.clone()),
            Self::InternalError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                msg.clone(),
            ),
        };

        let body = ErrorResponse {
            error: ErrorBody { code, message },
        };

        (status, Json(body)).into_response()
    }
}
