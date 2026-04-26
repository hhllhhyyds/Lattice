//! Lattice HTTP API server built on axum.
//!
//! Exposes the Lattice agent framework as a REST API. Supports multiple LLM
//! providers (Anthropic, OpenAI-compatible) controlled via feature flags.

mod api;
mod error;

use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use chrono::{DateTime, Utc};
pub use lattice_core::SessionId;
use serde::Serialize;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::api::types::SessionMetadata;
pub use crate::error::AppError;

/// Global shared application state.
pub struct AppState {
    /// Session store (currently MemoryStore, swappable via trait).
    pub store: Arc<dyn lattice_core::SessionStore>,
    /// Active ControlLoop task handles.
    pub active_runs: Arc<RwLock<HashMap<SessionId, RunHandle>>>,
    /// Server start time (for uptime reporting).
    pub started_at: DateTime<Utc>,
    /// Index of all known sessions (for listing).
    pub sessions: Arc<RwLock<Vec<SessionInfo>>>,
}

/// Metadata for a tracked session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Unique session identifier.
    pub session_id: SessionId,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// Optional user-supplied metadata.
    pub metadata: Option<SessionMetadata>,
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
        .nest("/v1", crate::api::v1_routes())
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
        sessions: Arc::new(RwLock::new(Vec::new())),
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

    #[tokio::test]
    async fn create_session_returns_201() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["sessionId"].is_string());
        assert_eq!(json["status"], "created");
        assert_eq!(json["eventCount"], 1);
    }

    #[tokio::test]
    async fn create_session_with_metadata() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"metadata":{"name":"test","tags":["a","b"]}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn list_sessions_contains_created() {
        let app = make_app();

        // Create a session first.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(create_resp.into_body(), 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = created["sessionId"].as_str().unwrap();

        // List sessions.
        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list_resp.into_body(), 1024)
            .await
            .unwrap();
        let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let sessions: Vec<&str> = list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["sessionId"].as_str().unwrap())
            .collect();
        assert!(sessions.contains(&session_id));
    }

    #[tokio::test]
    async fn list_sessions_empty_returns_empty_array() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/sessions")
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
        assert!(json["sessions"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_session_returns_correct_details() {
        let app = make_app();

        // Create.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(create_resp.into_body(), 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = created["sessionId"].as_str().unwrap();

        // Get details.
        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(get_resp.into_body(), 1024)
            .await
            .unwrap();
        let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(detail["sessionId"], session_id);
        assert_eq!(detail["status"], "created");
        assert_eq!(detail["eventCount"], 1);
        assert!(detail["latestEventId"].is_string());
    }

    #[tokio::test]
    async fn get_session_not_found_returns_404() {
        let app = make_app();
        let fake_id = "00000000-0000-0000-0000-000000000000";
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{fake_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "session_not_found");
    }

    #[tokio::test]
    async fn get_events_returns_session_created() {
        let app = make_app();

        // Create.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(create_resp.into_body(), 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = created["sessionId"].as_str().unwrap();

        // Get events.
        let events_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(events_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(events_resp.into_body(), 1024)
            .await
            .unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(evts["events"].as_array().unwrap().len(), 1);
        assert_eq!(evts["events"][0]["payload"]["type"], "sessionCreated");
        assert_eq!(evts["hasMore"], false);
    }

    #[tokio::test]
    async fn get_events_not_found_returns_404() {
        let app = make_app();
        let fake_id = "00000000-0000-0000-0000-000000000000";
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{fake_id}/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_events_with_limit() {
        let app = make_app();

        // Create.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(create_resp.into_body(), 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = created["sessionId"].as_str().unwrap();

        // Events with limit=0 should return 0 events but hasMore indicates more.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}/events?limit=0"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_events_with_actor_filter() {
        let app = make_app();

        // Create.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(create_resp.into_body(), 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = created["sessionId"].as_str().unwrap();

        // Filter by actor=System (should return SessionCreated).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}/events?actor=System"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(evts["events"].as_array().unwrap().len(), 1);

        // Filter by actor=LLM (should return 0 — SessionCreated is System).
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}/events?actor=LLM"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(evts["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_events_with_event_type_filter() {
        let app = make_app();

        // Create.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(create_resp.into_body(), 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = created["sessionId"].as_str().unwrap();

        // Filter by eventType=sessionCreated
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/sessions/{session_id}/events?eventType=sessionCreated"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(evts["events"].as_array().unwrap().len(), 1);
        assert_eq!(evts["events"][0]["payload"]["type"], "sessionCreated");

        // Filter by eventType=thinking (should return 0)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/sessions/{session_id}/events?eventType=thinking"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(evts["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_events_with_cursor_pagination() {
        let app = make_app();

        // Create.
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(create_resp.into_body(), 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = created["sessionId"].as_str().unwrap();

        // Query events to get the first event ID for cursor testing.
        let events_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(events_resp.into_body(), 1024)
            .await
            .unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let first_event_id = evts["events"][0]["eventId"].as_str().unwrap();

        // Get events after the first event (should be empty since there's only one event).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/sessions/{session_id}/events?after={first_event_id}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(evts["events"].as_array().unwrap().is_empty());
        assert!(!evts["hasMore"].as_bool().unwrap());
    }
}
