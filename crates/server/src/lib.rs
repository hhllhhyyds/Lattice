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
use lattice_mcp::{
    load_mcp_manager_from_env, register_mcp_tools, McpClientManager, McpConnectionSnapshot,
};
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
    /// Optional MCP manager backing registered MCP tools.
    pub mcp_manager: Option<Arc<McpClientManager>>,
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
///
/// All required values must be explicitly configured — neither the function
/// args nor the environment may be missing, and there are no built-in
/// defaults. Returns a descriptive error naming the first missing variable
/// so the caller (or HTTP request handler) can surface it to the user.
pub fn create_llm_client(
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<Arc<dyn LLMClient>, String> {
    let provider = provider
        .map(str::to_owned)
        .or_else(|| env::var("LATTICE_LLM_PROVIDER").ok())
        .ok_or_else(|| {
            "LATTICE_LLM_PROVIDER must be set (or pass `provider` in the request body)".to_string()
        })?;

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
        .ok_or_else(|| {
            "LATTICE_MODEL must be set (or pass `model` in the request body)".to_string()
        })?;
    let base_url = env::var("LATTICE_ANTHROPIC_API_BASE")
        .or_else(|_| env::var("ANTHROPIC_API_BASE"))
        .or_else(|_| env::var("LATTICE_API_BASE"))
        .map_err(|_| {
            "LATTICE_ANTHROPIC_API_BASE (or ANTHROPIC_API_BASE / LATTICE_API_BASE) must be set"
                .to_string()
        })?;

    let client =
        lattice_llm_anthropic::AnthropicClient::new(api_key, model).with_base_url(base_url);
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
        .or_else(|| env::var("LATTICE_OPENAI_MODEL").ok())
        .or_else(|| env::var("LATTICE_MODEL").ok())
        .ok_or_else(|| {
            "LATTICE_OPENAI_MODEL or LATTICE_MODEL must be set (or pass `model` in the request body)"
                .to_string()
        })?;
    let base_url = env::var("LATTICE_OPENAI_API_BASE")
        .or_else(|_| env::var("OPENAI_API_BASE"))
        .or_else(|_| env::var("LATTICE_API_BASE"))
        .map_err(|_| {
            "LATTICE_OPENAI_API_BASE (or OPENAI_API_BASE / LATTICE_API_BASE) must be set"
                .to_string()
        })?;

    let client = lattice_llm_openai::OpenAIClient::new(api_key, model).with_base_url(base_url);
    Ok(Arc::new(client))
}

#[cfg(not(feature = "openai"))]
fn create_openai_client(_model: Option<&str>) -> Result<Arc<dyn LLMClient>, String> {
    Err("openai provider is not enabled in this build".to_string())
}

/// Snapshot of the default LLM configuration the server would resolve from
/// the environment for requests that omit `provider`/`model`. Each field is
/// optional because configuration is no longer permitted to fall back to
/// built-in defaults — missing values are surfaced as `None` so the startup
/// banner can show "(not set)" instead of silently lying about what URL the
/// server would call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultLlmSummary {
    /// Provider identifier (`anthropic` or `openai`). `None` when
    /// `LATTICE_LLM_PROVIDER` is not set.
    pub provider: Option<String>,
    /// Model name that would be used. `None` when no provider-appropriate
    /// model variable is set.
    pub model: Option<String>,
    /// Base URL the client would call. `None` when no provider-appropriate
    /// base-URL variable is set.
    pub api_base: Option<String>,
}

/// Resolve the default LLM configuration from environment variables.
///
/// Mirrors the precedence chain of [`create_llm_client`] but reports a
/// summary instead of constructing the client. Unlike `create_llm_client`,
/// this function does not error on missing variables — it returns `None`
/// for any field that the user has not explicitly configured, so callers
/// (typically the startup banner) can render a diagnostic view.
pub fn default_llm_summary() -> DefaultLlmSummary {
    let provider = env::var("LATTICE_LLM_PROVIDER").ok();

    match provider.as_deref() {
        Some("anthropic") => DefaultLlmSummary {
            provider,
            model: env::var("LATTICE_MODEL").ok(),
            api_base: env::var("LATTICE_ANTHROPIC_API_BASE")
                .or_else(|_| env::var("ANTHROPIC_API_BASE"))
                .or_else(|_| env::var("LATTICE_API_BASE"))
                .ok(),
        },
        // Treat anything else as openai-compatible (matches create_llm_client).
        // When provider is unset we still resolve the openai-side vars so the
        // banner shows what is configured; selection is reported as `None`.
        _ => DefaultLlmSummary {
            provider,
            model: env::var("LATTICE_OPENAI_MODEL")
                .or_else(|_| env::var("LATTICE_MODEL"))
                .ok(),
            api_base: env::var("LATTICE_OPENAI_API_BASE")
                .or_else(|_| env::var("OPENAI_API_BASE"))
                .or_else(|_| env::var("LATTICE_API_BASE"))
                .ok(),
        },
    }
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
    /// Current MCP server connection snapshots.
    pub mcp_servers: Vec<McpConnectionSnapshot>,
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
        mcp_servers: state
            .mcp_manager
            .as_ref()
            .map(|manager| manager.list_status_snapshots())
            .unwrap_or_default(),
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
    new_state_with_mcp_components(
        store,
        Arc::new(EnvLlmClientFactory),
        Arc::new(ToolSet::with_defaults(sandbox)),
        None,
    )
}

/// Creates a new AppState using environment-backed MCP configuration when present.
pub async fn new_state_from_env(
    store: Arc<dyn lattice_core::SessionStore>,
) -> Result<AppState, String> {
    let sandbox: Arc<dyn Sandbox> = Arc::new(LocalSandbox::new());
    let mut tools = ToolSet::with_defaults(sandbox);
    let mcp_manager = load_mcp_manager_from_env().await?;
    if let Some(manager) = &mcp_manager {
        register_mcp_tools(&mut tools, Arc::clone(manager)).map_err(|err| err.to_string())?;
    }

    Ok(new_state_with_mcp_components(
        store,
        Arc::new(EnvLlmClientFactory),
        Arc::new(tools),
        mcp_manager,
    ))
}

/// Creates a new AppState with injectable runtime components.
pub fn new_state_with_components(
    store: Arc<dyn lattice_core::SessionStore>,
    llm_factory: Arc<dyn LlmClientFactory>,
    tools: Arc<ToolSet>,
) -> AppState {
    new_state_with_mcp_components(store, llm_factory, tools, None)
}

/// Creates a new AppState with injectable runtime components and optional MCP manager.
pub fn new_state_with_mcp_components(
    store: Arc<dyn lattice_core::SessionStore>,
    llm_factory: Arc<dyn LlmClientFactory>,
    tools: Arc<ToolSet>,
    mcp_manager: Option<Arc<McpClientManager>>,
) -> AppState {
    let event_hub = Arc::new(EventHub::new());
    let store: Arc<dyn lattice_core::SessionStore> =
        Arc::new(NotifyingStore::new(store, Arc::clone(&event_hub)));

    AppState {
        store,
        event_hub,
        llm_factory,
        tools,
        mcp_manager,
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
    use chrono::{Duration, SecondsFormat, Timelike};
    use lattice_core::{
        Actor, Decision, Event, EventFilter, EventId, EventPayload, LLMError, SessionId,
        StoreError, ToolDescription,
    };
    use std::sync::Mutex;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn make_app() -> Router {
        let store = Arc::new(lattice_store_memory::MemoryStore::new());
        router(new_state(store))
    }

    struct FixedStore {
        session_id: SessionId,
        events: Vec<Event>,
        last_filter: RwLock<Option<EventFilter>>,
    }

    impl FixedStore {
        fn new(session_id: SessionId, events: Vec<Event>) -> Self {
            Self {
                session_id,
                events,
                last_filter: RwLock::new(None),
            }
        }
    }

    #[async_trait]
    impl lattice_core::SessionStore for FixedStore {
        async fn create_session(&self) -> Result<SessionId, StoreError> {
            Ok(self.session_id)
        }

        async fn delete_session(&self, session_id: SessionId) -> Result<(), StoreError> {
            if session_id == self.session_id {
                Ok(())
            } else {
                Err(StoreError::SessionNotFound(session_id))
            }
        }

        async fn append_event(
            &self,
            _session_id: SessionId,
            _payload: EventPayload,
            _actor: Actor,
            _parent_event_id: Option<EventId>,
        ) -> Result<EventId, StoreError> {
            panic!("append_event not used in this test")
        }

        async fn get_events(
            &self,
            session_id: SessionId,
            filter: &EventFilter,
        ) -> Result<Vec<Event>, StoreError> {
            if session_id != self.session_id {
                return Err(StoreError::SessionNotFound(session_id));
            }

            *self.last_filter.write().await = Some(filter.clone());

            let mut events = self.events.clone();
            if let Some(actor) = filter.actor {
                events.retain(|event| event.actor == actor);
            }
            if let Some(payload_type) = filter.payload_type {
                events.retain(|event| {
                    let json = serde_json::to_value(&event.payload).ok();
                    json.as_ref()
                        .and_then(|value| value.get("type"))
                        .and_then(|value| value.as_str())
                        .is_some_and(|value| value == payload_type)
                });
            }
            if let Some(after_event_id) = filter.after_event_id {
                events = events
                    .into_iter()
                    .skip_while(|event| event.event_id != after_event_id)
                    .skip(1)
                    .collect();
            }
            if let Some(since) = filter.since {
                events.retain(|event| event.timestamp >= since);
            }
            if let Some(until) = filter.until {
                events.retain(|event| event.timestamp <= until);
            }
            if let Some(limit) = filter.limit {
                events.truncate(limit);
            }

            Ok(events)
        }

        async fn latest_event_id(
            &self,
            session_id: SessionId,
        ) -> Result<Option<EventId>, StoreError> {
            if session_id != self.session_id {
                return Err(StoreError::SessionNotFound(session_id));
            }

            Ok(self.events.last().map(|event| event.event_id))
        }
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
        assert!(html.contains("mcp-panel"));
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
        assert!(js.contains("loadMcpStatus"));
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
        assert!(json["mcp_servers"].is_array());
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
            std::env::set_var("LATTICE_OPENAI_API_BASE", "https://example.test/v1");
            std::env::remove_var("LATTICE_API_KEY");
            std::env::remove_var("OPENAI_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
        }

        let client = create_llm_client(Some("openai"), Some("gpt-4o"));
        assert!(client.is_ok());

        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("LATTICE_OPENAI_API_BASE");
        }
    }

    #[test]
    #[cfg(feature = "anthropic")]
    fn create_llm_client_uses_anthropic_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test");
            std::env::set_var("LATTICE_ANTHROPIC_API_BASE", "https://example.test");
            std::env::remove_var("LATTICE_API_KEY");
            std::env::remove_var("ANTHROPIC_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
        }

        let client = create_llm_client(Some("anthropic"), Some("claude-sonnet-4-20250514"));
        assert!(client.is_ok());

        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("LATTICE_ANTHROPIC_API_BASE");
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

    /// Strict mode: a missing `LATTICE_LLM_PROVIDER` (and no override) is a
    /// hard error rather than a silent fallback to "openai".
    #[test]
    fn create_llm_client_reports_missing_provider() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("LATTICE_LLM_PROVIDER");
        }

        let err = match create_llm_client(None, None) {
            Ok(_) => panic!("expected missing provider to fail"),
            Err(err) => err,
        };
        assert!(err.contains("LATTICE_LLM_PROVIDER"));
    }

    /// Strict mode: missing model env var is a hard error rather than the
    /// old `"gpt-4o"` / `"claude-sonnet-4-20250514"` fallback.
    #[test]
    #[cfg(feature = "openai")]
    fn create_llm_client_reports_missing_openai_model() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LATTICE_API_KEY", "sk-test");
            std::env::set_var("LATTICE_OPENAI_API_BASE", "https://example.test/v1");
            std::env::remove_var("LATTICE_OPENAI_MODEL");
            std::env::remove_var("LATTICE_MODEL");
        }

        let err = match create_llm_client(Some("openai"), None) {
            Ok(_) => panic!("expected missing model to fail"),
            Err(err) => err,
        };
        assert!(err.contains("LATTICE_OPENAI_MODEL") || err.contains("LATTICE_MODEL"));

        unsafe {
            std::env::remove_var("LATTICE_API_KEY");
            std::env::remove_var("LATTICE_OPENAI_API_BASE");
        }
    }

    /// Strict mode: missing api_base env var is a hard error rather than
    /// falling back to the client's internal default URL.
    #[test]
    #[cfg(feature = "openai")]
    fn create_llm_client_reports_missing_openai_api_base() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LATTICE_API_KEY", "sk-test");
            std::env::remove_var("LATTICE_OPENAI_API_BASE");
            std::env::remove_var("OPENAI_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
        }

        let err = match create_llm_client(Some("openai"), Some("gpt-4o")) {
            Ok(_) => panic!("expected missing api_base to fail"),
            Err(err) => err,
        };
        assert!(err.contains("API_BASE"));

        unsafe {
            std::env::remove_var("LATTICE_API_KEY");
        }
    }

    /// `LATTICE_OPENAI_API_BASE` must take precedence over the legacy
    /// `OPENAI_API_BASE` and the generic `LATTICE_API_BASE`, matching the
    /// convention used by the test suite and `.env.example`.
    #[test]
    #[cfg(feature = "openai")]
    fn create_llm_client_prefers_lattice_openai_api_base() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LATTICE_API_KEY", "sk-test");
            std::env::set_var("LATTICE_OPENAI_API_BASE", "https://primary.example/v1");
            std::env::set_var("OPENAI_API_BASE", "https://legacy.example/v1");
            std::env::set_var("LATTICE_API_BASE", "https://generic.example/v1");
        }

        // Success here only proves the precedence chain compiles cleanly; the
        // value itself is verified by integration tests, since OpenAIClient
        // does not expose its base URL. The important part is no panic and
        // no error on construction.
        let client = create_llm_client(Some("openai"), Some("gpt-4o"));
        assert!(client.is_ok(), "expected client construction to succeed");

        unsafe {
            std::env::remove_var("LATTICE_API_KEY");
            std::env::remove_var("LATTICE_OPENAI_API_BASE");
            std::env::remove_var("OPENAI_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
        }
    }

    /// `LATTICE_ANTHROPIC_API_BASE` must take precedence over `ANTHROPIC_API_BASE`
    /// and `LATTICE_API_BASE` for the same reason as the openai case above.
    #[test]
    #[cfg(feature = "anthropic")]
    fn create_llm_client_prefers_lattice_anthropic_api_base() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LATTICE_API_KEY", "sk-test");
            std::env::set_var("LATTICE_ANTHROPIC_API_BASE", "https://primary.example");
            std::env::set_var("ANTHROPIC_API_BASE", "https://legacy.example");
            std::env::set_var("LATTICE_API_BASE", "https://generic.example");
        }

        let client = create_llm_client(Some("anthropic"), Some("claude-sonnet-4-20250514"));
        assert!(client.is_ok());

        unsafe {
            std::env::remove_var("LATTICE_API_KEY");
            std::env::remove_var("LATTICE_ANTHROPIC_API_BASE");
            std::env::remove_var("ANTHROPIC_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
        }
    }

    /// For OpenAI, `LATTICE_OPENAI_MODEL` overrides the generic `LATTICE_MODEL`
    /// so a single `.env` can pin different defaults for each provider.
    #[test]
    #[cfg(feature = "openai")]
    fn create_llm_client_prefers_lattice_openai_model() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LATTICE_API_KEY", "sk-test");
            std::env::set_var("LATTICE_OPENAI_API_BASE", "https://example.test/v1");
            std::env::set_var("LATTICE_OPENAI_MODEL", "gpt-4o");
            std::env::set_var("LATTICE_MODEL", "deepseek-v4-flash");
        }

        // Explicit override via the function arg wins over env. Pass `None`
        // here so the env precedence is what we are testing.
        let client = create_llm_client(Some("openai"), None);
        assert!(client.is_ok());

        unsafe {
            std::env::remove_var("LATTICE_API_KEY");
            std::env::remove_var("LATTICE_OPENAI_API_BASE");
            std::env::remove_var("LATTICE_OPENAI_MODEL");
            std::env::remove_var("LATTICE_MODEL");
        }
    }

    /// The default summary for the openai provider must reflect the
    /// provider-prefixed env vars, with all fields returned as `Some`.
    #[test]
    fn default_llm_summary_resolves_openai_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LATTICE_LLM_PROVIDER", "openai");
            std::env::set_var("LATTICE_OPENAI_API_BASE", "https://primary.example/v1");
            std::env::set_var("LATTICE_OPENAI_MODEL", "gpt-4o-mini");
            std::env::remove_var("OPENAI_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
            std::env::remove_var("LATTICE_MODEL");
        }

        let summary = default_llm_summary();
        assert_eq!(summary.provider.as_deref(), Some("openai"));
        assert_eq!(summary.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(
            summary.api_base.as_deref(),
            Some("https://primary.example/v1")
        );

        unsafe {
            std::env::remove_var("LATTICE_LLM_PROVIDER");
            std::env::remove_var("LATTICE_OPENAI_API_BASE");
            std::env::remove_var("LATTICE_OPENAI_MODEL");
        }
    }

    /// Strict mode: with no env vars set the summary reports `None` for
    /// every field so the banner can show `(not set)` instead of silently
    /// claiming "https://api.openai.com/v1" is in use.
    #[test]
    fn default_llm_summary_reports_none_when_unconfigured() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("LATTICE_LLM_PROVIDER");
            std::env::remove_var("LATTICE_OPENAI_API_BASE");
            std::env::remove_var("OPENAI_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
            std::env::remove_var("LATTICE_OPENAI_MODEL");
            std::env::remove_var("LATTICE_MODEL");
        }

        let summary = default_llm_summary();
        assert_eq!(summary.provider, None);
        assert_eq!(summary.model, None);
        assert_eq!(summary.api_base, None);
    }

    /// Anthropic summary picks `LATTICE_ANTHROPIC_API_BASE` first and reads
    /// the model name from `LATTICE_MODEL`.
    #[test]
    fn default_llm_summary_resolves_anthropic_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("LATTICE_LLM_PROVIDER", "anthropic");
            std::env::set_var("LATTICE_ANTHROPIC_API_BASE", "http://10.0.20.110:3001");
            std::env::set_var("LATTICE_MODEL", "claude-sonnet-4-6");
            std::env::remove_var("ANTHROPIC_API_BASE");
            std::env::remove_var("LATTICE_API_BASE");
        }

        let summary = default_llm_summary();
        assert_eq!(summary.provider.as_deref(), Some("anthropic"));
        assert_eq!(summary.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(summary.api_base.as_deref(), Some("http://10.0.20.110:3001"));

        unsafe {
            std::env::remove_var("LATTICE_LLM_PROVIDER");
            std::env::remove_var("LATTICE_ANTHROPIC_API_BASE");
            std::env::remove_var("LATTICE_MODEL");
        }
    }

    #[tokio::test]
    async fn injected_state_components_are_used() {
        let store = Arc::new(lattice_store_memory::MemoryStore::new());
        let state =
            new_state_with_components(store, Arc::new(TestFactory), Arc::new(ToolSet::new()));

        assert_eq!(state.tools.len(), 0);
        assert!(state.mcp_manager.is_none());
        let client = state.llm_factory.create(None, None).unwrap();
        let decision = client.decide(&[], &[], "").await.unwrap();
        match decision {
            Decision::FinalAnswer { answer } => assert_eq!(answer, "done"),
            _ => panic!("expected final answer"),
        }
    }

    #[tokio::test]
    async fn health_reports_mcp_server_snapshots() {
        let mut configs = std::collections::HashMap::new();
        configs.insert(
            "http-remote".to_string(),
            lattice_mcp::McpServerConfig::Http(lattice_mcp::McpHttpServerConfig {
                url: "https://example.com/mcp".to_string(),
                bearer_token: None,
                headers: std::collections::HashMap::new(),
            }),
        );
        let mut manager = lattice_mcp::McpClientManager::new(configs);
        manager.connect_all().await;

        let state = new_state_with_mcp_components(
            Arc::new(lattice_store_memory::MemoryStore::new()),
            Arc::new(TestFactory),
            Arc::new(ToolSet::new()),
            Some(Arc::new(manager)),
        );
        let app = router(state);

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

        let body = axum::body::to_bytes(response.into_body(), 1024 * 16)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["mcp_servers"].as_array().unwrap().len(), 1);
        assert_eq!(json["mcp_servers"][0]["name"], "http-remote");
        assert_eq!(json["mcp_servers"][0]["state"], "failed");
        assert_eq!(json["mcp_servers"][0]["tool_count"], 0);
        assert_eq!(json["mcp_servers"][0]["resource_count"], 0);
    }

    #[tokio::test]
    async fn mcp_status_route_reports_disabled_when_unconfigured() {
        let app = make_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mcp")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024 * 16)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["enabled"], false);
        assert_eq!(json["serverCount"], 0);
        assert_eq!(json["connectedCount"], 0);
        assert_eq!(json["failedCount"], 0);
        assert!(json["servers"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mcp_status_route_reports_server_details() {
        let mut configs = std::collections::HashMap::new();
        configs.insert(
            "http-remote".to_string(),
            lattice_mcp::McpServerConfig::Http(lattice_mcp::McpHttpServerConfig {
                url: "https://example.com/mcp".to_string(),
                bearer_token: None,
                headers: std::collections::HashMap::new(),
            }),
        );
        let mut manager = lattice_mcp::McpClientManager::new(configs);
        manager.connect_all().await;

        let state = new_state_with_mcp_components(
            Arc::new(lattice_store_memory::MemoryStore::new()),
            Arc::new(TestFactory),
            Arc::new(ToolSet::new()),
            Some(Arc::new(manager)),
        );
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mcp")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024 * 16)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["enabled"], true);
        assert_eq!(json["serverCount"], 1);
        assert_eq!(json["connectedCount"], 0);
        assert_eq!(json["failedCount"], 1);
        assert_eq!(json["servers"][0]["name"], "http-remote");
        assert_eq!(json["servers"][0]["transport"], "http");
        assert_eq!(json["servers"][0]["state"], "failed");
        assert!(!json["servers"][0]["detail"].as_str().unwrap().is_empty());
        assert!(json["servers"][0]["tools"].as_array().unwrap().is_empty());
        assert!(json["servers"][0]["resources"]
            .as_array()
            .unwrap()
            .is_empty());
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

    #[tokio::test]
    async fn get_events_with_time_range_filters() {
        let base = Utc::now().with_nanosecond(0).unwrap();
        let session_id = SessionId::new_v4();
        let after_event_id = EventId::new_v4();
        let store = Arc::new(FixedStore::new(
            session_id,
            vec![
                Event {
                    event_id: EventId::new_v4(),
                    session_id,
                    timestamp: base,
                    actor: Actor::System,
                    payload: EventPayload::SessionCreated,
                    parent_event_id: None,
                },
                Event {
                    event_id: after_event_id,
                    session_id,
                    timestamp: base + Duration::seconds(10),
                    actor: Actor::Harness,
                    payload: EventPayload::UserMessage {
                        content: "first".into(),
                    },
                    parent_event_id: None,
                },
                Event {
                    event_id: EventId::new_v4(),
                    session_id,
                    timestamp: base + Duration::seconds(20),
                    actor: Actor::LLM,
                    payload: EventPayload::Thinking {
                        reasoning: "second".into(),
                        signature: None,
                    },
                    parent_event_id: None,
                },
                Event {
                    event_id: EventId::new_v4(),
                    session_id,
                    timestamp: base + Duration::seconds(30),
                    actor: Actor::LLM,
                    payload: EventPayload::FinalAnswer {
                        answer: "third".into(),
                    },
                    parent_event_id: None,
                },
            ],
        ));
        let state = new_state(store.clone());
        state.sessions.write().await.push(SessionInfo {
            session_id,
            created_at: base,
            metadata: None,
        });
        let app = router(state);
        let since = (base + Duration::seconds(15)).to_rfc3339_opts(SecondsFormat::Secs, true);
        let until = (base + Duration::seconds(25)).to_rfc3339_opts(SecondsFormat::Secs, true);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/sessions/{session_id}/events?after={after_event_id}&since={since}&until={until}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let events = evts["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["payload"]["type"], "thinking");
        assert!(!evts["hasMore"].as_bool().unwrap());
        let first_filter = store.last_filter.read().await.clone().unwrap();
        assert_eq!(first_filter.after_event_id, Some(after_event_id));
        assert_eq!(first_filter.since, Some(base + Duration::seconds(15)));
        assert_eq!(first_filter.until, Some(base + Duration::seconds(25)));
        assert_eq!(first_filter.limit, Some(101));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/sessions/{session_id}/events?since={since}&until={until}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let evts: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let events = evts["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["payload"]["type"], "thinking");
        let second_filter = store.last_filter.read().await.clone().unwrap();
        assert_eq!(second_filter.after_event_id, None);
        assert_eq!(second_filter.since, Some(base + Duration::seconds(15)));
        assert_eq!(second_filter.until, Some(base + Duration::seconds(25)));
        assert_eq!(second_filter.limit, Some(101));
    }

    // --- TDD tests for #107: Web UI Markdown Rendering ---

    /// index.html must load the `marked` markdown-parsing library.
    #[tokio::test]
    async fn web_ui_index_loads_marked_library() {
        let app = make_app();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            html.contains("marked"),
            "index.html must load the `marked` library"
        );
    }

    /// index.html must load DOMPurify so markdown HTML is sanitised before DOM injection.
    #[tokio::test]
    async fn web_ui_index_loads_dompurify() {
        let app = make_app();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            html.contains("DOMPurify"),
            "index.html must load DOMPurify for HTML sanitisation"
        );
    }

    /// app.js must call marked.parse() to convert markdown to HTML for assistant messages.
    #[tokio::test]
    async fn web_ui_script_uses_marked_parse() {
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
        let body = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let js = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            js.contains("marked.parse"),
            "app.js must call marked.parse() for markdown rendering"
        );
    }

    /// app.js must sanitise markdown-generated HTML via DOMPurify before injection.
    #[tokio::test]
    async fn web_ui_script_sanitises_with_dompurify() {
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
        let body = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let js = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            js.contains("DOMPurify.sanitize"),
            "app.js must sanitise rendered HTML with DOMPurify"
        );
    }

    /// app.js must inject assistant content into a div.markdown-body, not a bare <pre>.
    #[tokio::test]
    async fn web_ui_script_uses_markdown_body_div_for_assistant() {
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
        let body = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let js = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            js.contains("markdown-body"),
            "app.js must use a div with class 'markdown-body' for assistant message content"
        );
    }

    /// app.css must define a .markdown-body block for prose typography.
    #[tokio::test]
    async fn web_ui_styles_contains_markdown_body_class() {
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
        let body = axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap();
        let css = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            css.contains(".markdown-body"),
            "app.css must define .markdown-body styles for rendered markdown"
        );
    }
}
