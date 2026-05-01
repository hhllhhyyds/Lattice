//! Lattice HTTP API server built on axum.
//!
//! Exposes the Lattice agent framework as a REST API. Supports multiple LLM
//! providers (Anthropic, OpenAI-compatible) controlled via feature flags.

mod api;
mod error;
mod streaming;
mod ui;

use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use chrono::{DateTime, Utc};
pub use lattice_core::SessionId;
use lattice_core::{LLMClient, Sandbox};
use lattice_runtime::ControlLoop;
use lattice_sandbox_local::LocalSandbox;
use lattice_tools::ToolSet;
use serde::Serialize;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::api::types::SessionMetadata;
pub use crate::error::AppError;
use crate::streaming::{EventHub, NotifyingStore};

/// Global shared application state.
pub struct AppState {
    /// Session store (currently MemoryStore, swappable via trait).
    pub store: Arc<dyn lattice_core::SessionStore>,
    /// Broadcast hub for per-session event fan-out.
    pub event_hub: Arc<EventHub>,
    /// Factory for creating LLM clients per submitted run.
    pub llm_factory: Arc<dyn LlmClientFactory>,
    /// Tool registry used by agent runs.
    pub tools: Arc<ToolSet>,
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
    /// Unique ID for this run.
    pub run_id: String,
    /// Session ID for this run.
    pub session_id: SessionId,
    /// Current status of the run.
    pub status: RunStatus,
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// When the run finished.
    pub completed_at: Option<DateTime<Utc>>,
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

/// Factory for creating LLM clients for a run.
pub trait LlmClientFactory: Send + Sync {
    /// Create a client for the requested provider/model.
    fn create(
        &self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<Arc<dyn LLMClient>, String>;
}

/// Environment-backed LLM factory used by the production server.
pub struct EnvLlmClientFactory;

impl LlmClientFactory for EnvLlmClientFactory {
    fn create(
        &self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<Arc<dyn LLMClient>, String> {
        create_llm_client(provider, model)
    }
}

/// Create an LLM client from request overrides and environment variables.
pub fn create_llm_client(
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<Arc<dyn LLMClient>, String> {
    let provider = provider
        .map(str::to_owned)
        .or_else(|| env::var("LATTICE_LLM_PROVIDER").ok())
        .unwrap_or_else(|| "openai".to_string());

    match provider.as_str() {
        "anthropic" => create_anthropic_client(model),
        "openai" | "openai-compatible" => create_openai_client(model),
        other => Err(format!(
            "unsupported LLM provider '{other}' (expected 'anthropic' or 'openai')"
        )),
    }
}

#[cfg(feature = "anthropic")]
fn create_anthropic_client(model: Option<&str>) -> Result<Arc<dyn LLMClient>, String> {
    let api_key = env::var("ANTHROPIC_API_KEY")
        .or_else(|_| env::var("LATTICE_API_KEY"))
        .map_err(|_| "ANTHROPIC_API_KEY or LATTICE_API_KEY must be set".to_string())?;
    let model = model
        .map(str::to_owned)
        .or_else(|| env::var("LATTICE_MODEL").ok())
        .unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());

    let mut client = lattice_llm_anthropic::AnthropicClient::new(api_key, model);
    if let Ok(base_url) = env::var("ANTHROPIC_API_BASE").or_else(|_| env::var("LATTICE_API_BASE")) {
        client = client.with_base_url(base_url);
    }
    Ok(Arc::new(client))
}

#[cfg(not(feature = "anthropic"))]
fn create_anthropic_client(_model: Option<&str>) -> Result<Arc<dyn LLMClient>, String> {
    Err("anthropic provider is not enabled in this build".to_string())
}

#[cfg(feature = "openai")]
fn create_openai_client(model: Option<&str>) -> Result<Arc<dyn LLMClient>, String> {
    let api_key = env::var("OPENAI_API_KEY")
        .or_else(|_| env::var("LATTICE_API_KEY"))
        .map_err(|_| "OPENAI_API_KEY or LATTICE_API_KEY must be set".to_string())?;
    let model = model
        .map(str::to_owned)
        .or_else(|| env::var("LATTICE_MODEL").ok())
        .unwrap_or_else(|| "gpt-4o".to_string());

    let mut client = lattice_llm_openai::OpenAIClient::new(api_key, model);
    if let Ok(base_url) = env::var("OPENAI_API_BASE").or_else(|_| env::var("LATTICE_API_BASE")) {
        client = client.with_base_url(base_url);
    }
    Ok(Arc::new(client))
}

#[cfg(not(feature = "openai"))]
fn create_openai_client(_model: Option<&str>) -> Result<Arc<dyn LLMClient>, String> {
    Err("openai provider is not enabled in this build".to_string())
}

/// Spawn a real ControlLoop run and update active run state on completion.
pub async fn spawn_control_loop_run(
    state: Arc<AppState>,
    session_id: SessionId,
    run_id: String,
    started_at: DateTime<Utc>,
    llm: Arc<dyn LLMClient>,
    system_prompt: String,
    max_iterations: usize,
) -> tokio::task::AbortHandle {
    let store = Arc::clone(&state.store);
    let tools = Arc::clone(&state.tools);
    let active_runs = Arc::clone(&state.active_runs);
    let run_id_for_task = run_id.clone();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();

    let worker_handle = tokio::spawn(async move {
        let _ = start_rx.await;
        let control_loop =
            ControlLoop::with_options(store, llm, tools, system_prompt, max_iterations);
        match control_loop.run(session_id).await {
            Ok(_) => RunStatus::Completed,
            Err(err) => RunStatus::Failed(err.to_string()),
        }
    });

    let abort_handle = worker_handle.abort_handle();
    {
        let mut runs = state.active_runs.write().await;
        runs.insert(
            session_id,
            RunHandle {
                run_id,
                session_id,
                status: RunStatus::Running,
                started_at,
                completed_at: None,
                abort_handle: abort_handle.clone(),
            },
        );
    }

    tokio::spawn(async move {
        let status = match worker_handle.await {
            Ok(status) => status,
            Err(err) if err.is_cancelled() => RunStatus::Failed("run aborted".into()),
            Err(err) => RunStatus::Failed(format!("run task failed to join: {err}")),
        };

        let mut runs = active_runs.write().await;
        if let Some(handle) = runs.get_mut(&session_id) {
            if handle.run_id == run_id_for_task {
                handle.status = status;
                handle.completed_at = Some(Utc::now());
            }
        }
    });

    let _ = start_tx.send(());

    abort_handle
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
        .route("/", get(ui::index))
        .route("/ui/app.css", get(ui::styles))
        .route("/ui/app.js", get(ui::script))
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
    let sandbox: Arc<dyn Sandbox> = Arc::new(LocalSandbox::new());
    new_state_with_components(
        store,
        Arc::new(EnvLlmClientFactory),
        Arc::new(ToolSet::with_defaults(sandbox)),
    )
}

/// Creates a new AppState with injectable runtime components.
pub fn new_state_with_components(
    store: Arc<dyn lattice_core::SessionStore>,
    llm_factory: Arc<dyn LlmClientFactory>,
    tools: Arc<ToolSet>,
) -> AppState {
    let event_hub = Arc::new(EventHub::new());
    let store: Arc<dyn lattice_core::SessionStore> =
        Arc::new(NotifyingStore::new(store, Arc::clone(&event_hub)));

    AppState {
        store,
        event_hub,
        llm_factory,
        tools,
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
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use lattice_core::{Decision, Event, LLMError, ToolDescription};
    use std::sync::Mutex;
    use tower::ServiceExt;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn make_app() -> Router {
        let store = Arc::new(lattice_store_memory::MemoryStore::new());
        router(new_state(store))
    }

    struct TestLlm {
        result: Result<Decision, LLMError>,
    }

    #[async_trait]
    impl LLMClient for TestLlm {
        async fn decide(
            &self,
            _history: &[Event],
            _available_tools: &[ToolDescription],
            _system_prompt: &str,
        ) -> Result<Decision, LLMError> {
            self.result.clone()
        }
    }

    struct TestFactory;

    impl LlmClientFactory for TestFactory {
        fn create(
            &self,
            _provider: Option<&str>,
            _model: Option<&str>,
        ) -> Result<Arc<dyn LLMClient>, String> {
            Ok(Arc::new(TestLlm {
                result: Ok(Decision::FinalAnswer {
                    answer: "done".into(),
                }),
            }))
        }
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
    async fn root_serves_web_ui() {
        let app = make_app();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("Lattice Console"));
        assert!(html.contains("/ui/app.js"));
    }

    #[tokio::test]
    async fn web_ui_styles_route_serves_css() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ui/app.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/css; charset=utf-8"
        );

        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let css = String::from_utf8(body.to_vec()).unwrap();
        assert!(css.contains(".app-shell"));
        assert!(css.contains(".status-chip"));
    }

    #[tokio::test]
    async fn web_ui_script_route_serves_javascript() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ui/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/javascript; charset=utf-8"
        );

        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let js = String::from_utf8(body.to_vec()).unwrap();
        assert!(js.contains("loadSessions"));
        assert!(js.contains("sendMessage"));
        assert!(js.contains("deleteSession"));
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

    #[test]
    fn create_llm_client_rejects_unknown_provider() {
        let err = match create_llm_client(Some("unknown"), Some("model")) {
            Ok(_) => panic!("expected unknown provider to fail"),
            Err(err) => err,
        };
        assert!(err.contains("unsupported LLM provider"));
    }

    #[test]
    #[cfg(feature = "openai")]
    fn create_llm_client_uses_openai_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "sk-test");
            std::env::remove_var("LATTICE_API_KEY");
            std::env::remove_var("OPENAI_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
        }

        let client = create_llm_client(Some("openai"), Some("gpt-4o"));
        assert!(client.is_ok());

        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    #[test]
    #[cfg(feature = "anthropic")]
    fn create_llm_client_uses_anthropic_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test");
            std::env::remove_var("LATTICE_API_KEY");
            std::env::remove_var("ANTHROPIC_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
        }

        let client = create_llm_client(Some("anthropic"), Some("claude-sonnet-4-20250514"));
        assert!(client.is_ok());

        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
    }

    #[test]
    #[cfg(feature = "openai")]
    fn create_llm_client_reports_missing_openai_key() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("LATTICE_API_KEY");
        }

        let err = match create_llm_client(Some("openai"), Some("gpt-4o")) {
            Ok(_) => panic!("expected missing API key to fail"),
            Err(err) => err,
        };
        assert!(err.contains("OPENAI_API_KEY"));
    }

    #[tokio::test]
    async fn injected_state_components_are_used() {
        let store = Arc::new(lattice_store_memory::MemoryStore::new());
        let state =
            new_state_with_components(store, Arc::new(TestFactory), Arc::new(ToolSet::new()));

        assert_eq!(state.tools.len(), 0);
        let client = state.llm_factory.create(None, None).unwrap();
        let decision = client.decide(&[], &[], "").await.unwrap();
        match decision {
            Decision::FinalAnswer { answer } => assert_eq!(answer, "done"),
            _ => panic!("expected final answer"),
        }
    }

    #[tokio::test]
    async fn spawned_control_loop_registers_run_handle() {
        let store: Arc<dyn lattice_core::SessionStore> =
            Arc::new(lattice_store_memory::MemoryStore::new());
        let session_id = store.create_session().await.unwrap();
        let state = Arc::new(new_state_with_components(
            Arc::clone(&store),
            Arc::new(TestFactory),
            Arc::new(ToolSet::new()),
        ));
        let run_id = "run-complete".to_string();
        let started_at = Utc::now();
        let llm: Arc<dyn LLMClient> = Arc::new(TestLlm {
            result: Ok(Decision::FinalAnswer {
                answer: "done".into(),
            }),
        });

        spawn_control_loop_run(
            Arc::clone(&state),
            session_id,
            run_id.clone(),
            started_at,
            llm,
            "prompt".into(),
            5,
        )
        .await;

        let runs = state.active_runs.read().await;
        let handle = runs.get(&session_id).unwrap();
        assert_eq!(handle.run_id, run_id);
        assert!(matches!(
            handle.status,
            RunStatus::Running | RunStatus::Completed
        ));
    }

    #[tokio::test]
    async fn spawned_control_loop_updates_failed_status() {
        let store: Arc<dyn lattice_core::SessionStore> =
            Arc::new(lattice_store_memory::MemoryStore::new());
        let session_id = store.create_session().await.unwrap();
        let state = Arc::new(new_state_with_components(
            Arc::clone(&store),
            Arc::new(TestFactory),
            Arc::new(ToolSet::new()),
        ));
        let llm: Arc<dyn LLMClient> = Arc::new(TestLlm {
            result: Err(LLMError::RequestFailed("boom".into())),
        });

        spawn_control_loop_run(
            Arc::clone(&state),
            session_id,
            "run-fail".into(),
            Utc::now(),
            llm,
            "prompt".into(),
            5,
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let runs = state.active_runs.read().await;
        let handle = runs.get(&session_id).unwrap();
        match &handle.status {
            RunStatus::Failed(message) => assert!(message.contains("boom")),
            _ => panic!("expected failed run"),
        }
        assert!(handle.completed_at.is_some());
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
