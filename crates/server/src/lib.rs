//! Lattice HTTP API server built on axum.
//!
//! Exposes the Lattice agent framework as a REST API. Supports multiple LLM
//! providers (Anthropic, OpenAI-compatible) controlled via feature flags.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use chrono::{DateTime, Utc};
pub use lattice_core::SessionId;
use serde::Serialize;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Global shared application state.
pub struct AppState {
    /// Session store (currently MemoryStore, swappable via trait).
    pub store: Arc<dyn lattice_core::SessionStore>,
    /// Active ControlLoop task handles.
    pub active_runs: Arc<RwLock<HashMap<SessionId, RunHandle>>>,
    /// Server start time (for uptime reporting).
    pub started_at: DateTime<Utc>,
}

/// Handle for an in-flight agent run.
pub struct RunHandle {
    /// Session ID for this run.
    pub session_id: SessionId,
    /// Current status of the run.
    pub status: RunStatus,
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// Handle used to abort the run task.
    pub abort_handle: tokio::task::AbortHandle,
}

/// Status of an agent run.
pub enum RunStatus {
    /// Run is currently executing.
    Running,
    /// Run completed successfully.
    Completed,
    /// Run failed with an error message.
    Failed(String),
}

/// Health check response body.
#[derive(Serialize)]
pub struct HealthResponse {
    /// Always "ok" when the server is responding.
    pub status: &'static str,
    /// Server version from Cargo.toml.
    pub version: &'static str,
    /// Seconds since server started.
    pub uptime_seconds: i64,
    /// Which LLM providers are compiled in.
    pub features: serde_json::Value,
}

/// Returns which LLM providers are enabled at compile time.
fn enabled_features() -> serde_json::Value {
    serde_json::json!({
        "anthropic": cfg!(feature = "anthropic"),
        "openai": cfg!(feature = "openai"),
    })
}

/// Health check handler: GET /health
async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = Utc::now()
        .signed_duration_since(state.started_at)
        .num_seconds();

    let response = HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: uptime,
        features: enabled_features(),
    };

    (StatusCode::OK, Json(response))
}

/// Builds the application router with all routes and middleware.
fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        // Future tasks will add:
        // .nest("/v1", v1_routes())
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(Arc::new(state))
}

/// Creates a new AppState with the given session store.
pub fn new_state(store: Arc<dyn lattice_core::SessionStore>) -> AppState {
    AppState {
        store,
        active_runs: Arc::new(RwLock::new(HashMap::new())),
        started_at: Utc::now(),
    }
}

/// Returns the router configured with AppState.
pub fn router(state: AppState) -> Router {
    app(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn make_app() -> Router {
        let store = Arc::new(lattice_store_memory::MemoryStore::new());
        router(new_state(store))
    }

    #[tokio::test]
    async fn health_returns_200() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_response_body_is_valid_json() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert!(json["version"].is_string());
        assert!(json["uptime_seconds"].is_number());
        assert!(json["features"].is_object());
        assert!(json["features"]["anthropic"].is_boolean());
        assert!(json["features"]["openai"].is_boolean());
    }

    #[tokio::test]
    async fn health_unknown_path_returns_404() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health/unknown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
