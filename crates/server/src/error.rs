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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn test_session_not_found_into_response() {
        let id = lattice_core::SessionId::new_v4();
        let error = AppError::SessionNotFound(id);
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "session_not_found");
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not exist"));
    }

    #[tokio::test]
    async fn test_invalid_request_into_response() {
        let error = AppError::InvalidRequest("missing field 'name'".into());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "invalid_request");
        assert_eq!(json["error"]["message"], "missing field 'name'");
    }

    #[tokio::test]
    async fn test_internal_error_into_response() {
        let error = AppError::InternalError("database connection failed".into());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "internal_error");
        assert_eq!(json["error"]["message"], "database connection failed");
    }

    #[test]
    fn test_error_display() {
        let id = lattice_core::SessionId::new_v4();
        assert_eq!(
            AppError::SessionNotFound(id).to_string(),
            format!("Session with id {id} does not exist")
        );
        assert_eq!(
            AppError::InvalidRequest("bad input".into()).to_string(),
            "bad input"
        );
        assert_eq!(
            AppError::InternalError("something went wrong".into()).to_string(),
            "something went wrong"
        );
    }
}
