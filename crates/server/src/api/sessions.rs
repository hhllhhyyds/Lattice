//! Session API handlers.

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;

use async_stream::stream;
use axum::{
    extract::{Path, Query, State},
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use lattice_core::{Actor, EventFilter, EventId, SessionId};
use lattice_mcp::McpConnectionState;
use serde::Deserialize;

use crate::api::types::*;
use crate::error::AppError;
use crate::{AppState, SessionInfo};

/// Query parameters for listing events.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventQuery {
    /// Filter by actor type.
    pub actor: Option<Actor>,
    /// Filter by event payload type name.
    pub event_type: Option<String>,
    /// Return events after this event id (cursor-based pagination).
    pub after: Option<EventId>,
    /// Maximum number of events to return (default 100).
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Query parameters for SSE session streaming.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamQuery {
    /// Replay events after this event id.
    pub after: Option<EventId>,
    /// Whether to emit existing history before subscribing to live events.
    pub include_history: Option<bool>,
}

fn default_limit() -> usize {
    100
}

/// Registers all /v1 session routes.
pub fn v1_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sessions", post(create_session).get(list_sessions))
        .route("/sessions/{id}", get(get_session).delete(delete_session))
        .route("/mcp", get(get_mcp_status))
        .route("/sessions/{id}/events", get(get_events))
        .route("/sessions/{id}/stream", get(session_stream))
        .route(
            "/sessions/{id}/messages",
            post(submit_message).get(get_messages),
        )
        .route("/sessions/{id}/run", post(trigger_run))
        .route("/sessions/{id}/status", get(get_status))
}

/// GET /v1/mcp - returns MCP connection status and discovered capabilities.
async fn get_mcp_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<McpStatusResponse>, AppError> {
    let statuses = state
        .mcp_manager
        .as_ref()
        .map(|manager| manager.list_statuses())
        .unwrap_or_default();

    let connected_count = statuses
        .iter()
        .filter(|status| status.state == McpConnectionState::Connected)
        .count();
    let failed_count = statuses
        .iter()
        .filter(|status| status.state == McpConnectionState::Failed)
        .count();

    Ok(Json(McpStatusResponse {
        enabled: state.mcp_manager.is_some(),
        server_count: statuses.len(),
        connected_count,
        failed_count,
        servers: statuses
            .into_iter()
            .map(|status| McpServerStatusResponse {
                name: status.name,
                state: status.state,
                transport: status.transport,
                detail: status.detail,
                tool_count: status.tools.len(),
                resource_count: status.resources.len(),
                tools: status
                    .tools
                    .into_iter()
                    .map(|tool| McpToolSummary {
                        server_name: tool.server_name,
                        name: tool.name,
                        description: tool.description,
                    })
                    .collect(),
                resources: status
                    .resources
                    .into_iter()
                    .map(|resource| McpResourceSummary {
                        server_name: resource.server_name,
                        name: resource.name,
                        uri: resource.uri,
                        description: resource.description,
                    })
                    .collect(),
            })
            .collect(),
    }))
}

/// DELETE /v1/sessions/:id — deletes a session and all of its events.
async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
) -> Result<axum::http::StatusCode, AppError> {
    {
        let sessions = state.sessions.read().await;
        if !sessions.iter().any(|s| s.session_id == session_id) {
            return Err(AppError::SessionNotFound(session_id));
        }
    }

    {
        let mut runs = state.active_runs.write().await;
        if let Some(handle) = runs.remove(&session_id) {
            handle.abort_handle.abort();
        }
    }

    state
        .store
        .delete_session(session_id)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    {
        let mut sessions = state.sessions.write().await;
        sessions.retain(|session| session.session_id != session_id);
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// POST /v1/sessions — creates a new session.
async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(axum::http::StatusCode, Json<SessionResponse>), AppError> {
    // Create session via the store.
    let session_id = state
        .store
        .create_session()
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    let created_at = Utc::now();
    let metadata = req.metadata;

    // Register in the session index.
    {
        let mut sessions = state.sessions.write().await;
        sessions.push(SessionInfo {
            session_id,
            created_at,
            metadata: metadata.clone(),
        });
    }

    let response = SessionResponse {
        session_id,
        created_at,
        metadata,
        status: SessionStatus::Created,
        event_count: 1,
        run_info: None,
        latest_event_id: None,
    };

    Ok((axum::http::StatusCode::CREATED, Json(response)))
}

/// GET /v1/sessions — lists all known sessions.
async fn list_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SessionListResponse>, AppError> {
    let sessions = state.sessions.read().await;

    let mut responses = Vec::with_capacity(sessions.len());
    for info in sessions.iter() {
        let event_count = state
            .store
            .get_events(info.session_id, &EventFilter::default())
            .await
            .map(|evts| evts.len())
            .unwrap_or(0);

        let latest_event_id = state
            .store
            .latest_event_id(info.session_id)
            .await
            .ok()
            .flatten();

        let status = session_status_from_index(&state.active_runs, info.session_id).await;

        responses.push(SessionResponse {
            session_id: info.session_id,
            created_at: info.created_at,
            metadata: info.metadata.clone(),
            status,
            event_count,
            run_info: None,
            latest_event_id,
        });
    }

    Ok(Json(SessionListResponse {
        sessions: responses,
    }))
}

/// GET /v1/sessions/:id — returns session details.
async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<SessionResponse>, AppError> {
    // Look up in the index.
    let info = {
        let sessions = state.sessions.read().await;
        sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .cloned()
    };

    let info = info.ok_or(AppError::SessionNotFound(session_id))?;

    let event_count = state
        .store
        .get_events(session_id, &EventFilter::default())
        .await
        .map(|evts| evts.len())
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    let latest_event_id = state.store.latest_event_id(session_id).await.ok().flatten();

    let (status, run_info) = run_status_and_info(&state.active_runs, session_id).await;

    Ok(Json(SessionResponse {
        session_id,
        created_at: info.created_at,
        metadata: info.metadata,
        status,
        event_count,
        run_info,
        latest_event_id,
    }))
}

/// GET /v1/sessions/:id/events — returns events for a session.
async fn get_events(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
    Query(query): Query<EventQuery>,
) -> Result<Json<EventListResponse>, AppError> {
    // Verify session exists.
    {
        let sessions = state.sessions.read().await;
        if !sessions.iter().any(|s| s.session_id == session_id) {
            return Err(AppError::SessionNotFound(session_id));
        }
    }

    // Build the store filter (actor only — payload_type/after/limit are post-filtered in memory).
    let mut filter = EventFilter::default();
    if let Some(actor) = query.actor {
        filter.actor = Some(actor);
    }

    let all_events = state
        .store
        .get_events(session_id, &filter)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    // Post-filter by event type (payload_type is not in EventFilter yet).
    // The serialized tag matches serde's rename_all = "camelCase" on the enum.
    let all_events = if let Some(ref et) = query.event_type {
        all_events
            .into_iter()
            .filter(|e| {
                let type_name = match &e.payload {
                    lattice_core::EventPayload::SessionCreated => "sessionCreated",
                    lattice_core::EventPayload::UserMessage { .. } => "userMessage",
                    lattice_core::EventPayload::Thinking { .. } => "thinking",
                    lattice_core::EventPayload::ToolCallRequested { .. } => "toolCallRequested",
                    lattice_core::EventPayload::ToolCallResult { .. } => "toolCallResult",
                    lattice_core::EventPayload::ToolCallError { .. } => "toolCallError",
                    lattice_core::EventPayload::FinalAnswer { .. } => "finalAnswer",
                    lattice_core::EventPayload::StateChange { .. } => "stateChange",
                };
                type_name == et
            })
            .collect::<Vec<_>>()
    } else {
        all_events
    };

    // Post-filter by `after` cursor.
    let events: Vec<EventResponse> = if let Some(after_id) = query.after {
        all_events
            .into_iter()
            .skip_while(|e| e.event_id != after_id)
            .skip(1) // skip the `after` event itself
            .take(query.limit + 1)
            .map(EventResponse::from)
            .collect()
    } else {
        all_events
            .into_iter()
            .take(query.limit + 1)
            .map(EventResponse::from)
            .collect()
    };

    let has_more = events.len() > query.limit;
    let events = events.into_iter().take(query.limit).collect::<Vec<_>>();

    Ok(Json(EventListResponse { events, has_more }))
}

/// GET /v1/sessions/:id/stream — returns an SSE stream for session events.
async fn session_stream(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl futures_core::Stream<Item = Result<SseEvent, Infallible>>>, AppError> {
    {
        let sessions = state.sessions.read().await;
        if !sessions.iter().any(|s| s.session_id == session_id) {
            return Err(AppError::SessionNotFound(session_id));
        }
    }

    let mut receiver = state.event_hub.subscribe(session_id).await;
    let include_history = query.include_history.unwrap_or(false);
    let history = if include_history {
        let all_events = state
            .store
            .get_events(session_id, &EventFilter::default())
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        if let Some(after) = query.after {
            let mut seen_after = false;
            all_events
                .into_iter()
                .filter(|event| {
                    if seen_after {
                        true
                    } else if event.event_id == after {
                        seen_after = true;
                        false
                    } else {
                        false
                    }
                })
                .collect::<Vec<_>>()
        } else {
            all_events
        }
    } else {
        Vec::new()
    };

    let stream = stream! {
        let mut seen_ids = history
            .iter()
            .map(|event| event.event_id.to_string())
            .collect::<HashSet<_>>();

        for event in history {
            let terminal = is_terminal_event(&event.payload);
            yield Ok(to_session_sse_event(&event));
            if terminal {
                yield Ok(done_event(session_id));
                return;
            }
        }

        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if !seen_ids.insert(event.event_id.to_string()) {
                        continue;
                    }

                    let terminal = is_terminal_event(&event.payload);
                    yield Ok(to_session_sse_event(&event));
                    if terminal {
                        yield Ok(done_event(session_id));
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    ))
}

// --- Internal helpers ---

/// Derives the session status from active_runs.
async fn session_status_from_index(
    active_runs: &std::sync::Arc<
        tokio::sync::RwLock<std::collections::HashMap<SessionId, crate::RunHandle>>,
    >,
    session_id: SessionId,
) -> SessionStatus {
    let runs = active_runs.read().await;
    match runs.get(&session_id) {
        Some(handle) => match &handle.status {
            crate::RunStatus::Running => SessionStatus::Running,
            crate::RunStatus::Completed => SessionStatus::Completed,
            crate::RunStatus::Failed(_) => SessionStatus::Failed,
        },
        None => SessionStatus::Created,
    }
}

/// Returns the session status and run info if the run is active.
async fn run_status_and_info(
    active_runs: &std::sync::Arc<
        tokio::sync::RwLock<std::collections::HashMap<SessionId, crate::RunHandle>>,
    >,
    session_id: SessionId,
) -> (SessionStatus, Option<RunInfo>) {
    let runs = active_runs.read().await;
    match runs.get(&session_id) {
        Some(handle) => {
            let status = match &handle.status {
                crate::RunStatus::Running => SessionStatus::Running,
                crate::RunStatus::Completed => SessionStatus::Completed,
                crate::RunStatus::Failed(_) => SessionStatus::Failed,
            };
            let run_status = match &handle.status {
                crate::RunStatus::Running => RunStatus::Running,
                crate::RunStatus::Completed => RunStatus::Completed,
                crate::RunStatus::Failed(_) => RunStatus::Failed,
            };
            let run_info = RunInfo {
                started_at: handle.started_at,
                status: run_status,
            };
            (status, Some(run_info))
        }
        None => (SessionStatus::Created, None),
    }
}

// --- Task 16: Agent Run API handlers ---

/// POST /v1/sessions/:id/messages — submit message and trigger agent execution.
async fn submit_message(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
    Json(req): Json<SubmitMessageRequest>,
) -> Result<(axum::http::StatusCode, Json<SubmitMessageResponse>), AppError> {
    // Verify session exists.
    {
        let sessions = state.sessions.read().await;
        if !sessions.iter().any(|s| s.session_id == session_id) {
            return Err(AppError::SessionNotFound(session_id));
        }
    }

    // Validate request.
    if req.content.trim().is_empty() {
        return Err(AppError::InvalidRequest(
            "content field is required and cannot be empty".into(),
        ));
    }

    // Check for concurrent run.
    {
        let runs = state.active_runs.read().await;
        let has_running_handle = matches!(
            runs.get(&session_id).map(|handle| &handle.status),
            Some(crate::RunStatus::Running)
        );
        drop(runs);

        if has_running_handle && !session_has_terminal_event(&state, session_id).await? {
            return Err(AppError::Conflict(
                "Session already has a running task".into(),
            ));
        }
    }

    let llm = state
        .llm_factory
        .create(req.provider.as_deref(), req.model.as_deref())
        .map_err(AppError::InvalidRequest)?;

    // Append UserMessage event.
    state
        .store
        .append_event(
            session_id,
            lattice_core::EventPayload::UserMessage {
                content: req.content.clone(),
            },
            Actor::Harness,
            None,
        )
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    // Register a RunHandle (for now, just mark as running).
    let run_id = uuid::Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now();
    let system_prompt = req.system_prompt.unwrap_or_else(|| {
        "You are a helpful agent. You can execute shell commands using the available shell tool. \
         After getting tool results, provide a clear final answer to the user."
            .to_string()
    });
    let max_iterations = req.max_iterations.unwrap_or(50);
    crate::spawn_control_loop_run(
        Arc::clone(&state),
        session_id,
        run_id.clone(),
        started_at,
        llm,
        system_prompt,
        max_iterations,
    )
    .await;

    let response = SubmitMessageResponse {
        session_id,
        run_id,
        status: "running",
        message: "Agent task started",
    };

    Ok((axum::http::StatusCode::ACCEPTED, Json(response)))
}

/// POST /v1/sessions/:id/run — trigger agent execution for existing session events.
async fn trigger_run(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
    Json(req): Json<RunRequest>,
) -> Result<(axum::http::StatusCode, Json<SubmitMessageResponse>), AppError> {
    // Verify session exists.
    {
        let sessions = state.sessions.read().await;
        if !sessions.iter().any(|s| s.session_id == session_id) {
            return Err(AppError::SessionNotFound(session_id));
        }
    }

    // Check for concurrent run.
    {
        let runs = state.active_runs.read().await;
        let has_running_handle = matches!(
            runs.get(&session_id).map(|handle| &handle.status),
            Some(crate::RunStatus::Running)
        );
        drop(runs);

        if has_running_handle && !session_has_terminal_event(&state, session_id).await? {
            return Err(AppError::Conflict(
                "Session already has a running task".into(),
            ));
        }
    }

    let llm = state
        .llm_factory
        .create(req.provider.as_deref(), req.model.as_deref())
        .map_err(AppError::InvalidRequest)?;

    let run_id = uuid::Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now();
    let system_prompt = req.system_prompt.unwrap_or_else(|| {
        "You are a helpful agent. You can execute shell commands using the available shell tool. \
         After getting tool results, provide a clear final answer to the user."
            .to_string()
    });
    let max_iterations = req.max_iterations.unwrap_or(50);

    crate::spawn_control_loop_run(
        Arc::clone(&state),
        session_id,
        run_id.clone(),
        started_at,
        llm,
        system_prompt,
        max_iterations,
    )
    .await;

    let response = SubmitMessageResponse {
        session_id,
        run_id,
        status: "running",
        message: "Agent task started",
    };

    Ok((axum::http::StatusCode::ACCEPTED, Json(response)))
}

/// GET /v1/sessions/:id/messages — get conversation history.
async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<MessagesResponse>, AppError> {
    // Verify session exists.
    {
        let sessions = state.sessions.read().await;
        if !sessions.iter().any(|s| s.session_id == session_id) {
            return Err(AppError::SessionNotFound(session_id));
        }
    }

    // Get all events.
    let events = state
        .store
        .get_events(session_id, &lattice_core::EventFilter::default())
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    // Extract UserMessage and FinalAnswer events.
    let mut messages = Vec::new();
    for event in events {
        match event.payload {
            lattice_core::EventPayload::UserMessage { content } => {
                messages.push(Message {
                    role: "user",
                    content,
                    timestamp: event.timestamp,
                });
            }
            lattice_core::EventPayload::FinalAnswer { answer } => {
                messages.push(Message {
                    role: "assistant",
                    content: answer,
                    timestamp: event.timestamp,
                });
            }
            _ => {}
        }
    }

    Ok(Json(MessagesResponse { messages }))
}

/// GET /v1/sessions/:id/status — query execution status.
async fn get_status(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<StatusResponse>, AppError> {
    // Verify session exists.
    {
        let sessions = state.sessions.read().await;
        if !sessions.iter().any(|s| s.session_id == session_id) {
            return Err(AppError::SessionNotFound(session_id));
        }
    }

    // Get event count.
    let events = state
        .store
        .get_events(session_id, &lattice_core::EventFilter::default())
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;
    let event_count = events.len();

    // Get latest event.
    let latest_event = events.last().map(|e| {
        let payload_type = match &e.payload {
            lattice_core::EventPayload::SessionCreated => "sessionCreated",
            lattice_core::EventPayload::UserMessage { .. } => "userMessage",
            lattice_core::EventPayload::Thinking { .. } => "thinking",
            lattice_core::EventPayload::ToolCallRequested { .. } => "toolCallRequested",
            lattice_core::EventPayload::ToolCallResult { .. } => "toolCallResult",
            lattice_core::EventPayload::ToolCallError { .. } => "toolCallError",
            lattice_core::EventPayload::FinalAnswer { .. } => "finalAnswer",
            lattice_core::EventPayload::StateChange { .. } => "stateChange",
        };
        LatestEventInfo {
            event_id: e.event_id,
            actor: e.actor,
            payload_type: payload_type.to_string(),
            timestamp: e.timestamp,
        }
    });

    let completed_at_from_events = events
        .last()
        .and_then(|event| is_terminal_event(&event.payload).then_some(event.timestamp));

    // Check run status.
    let (run_status, run_started_at, run_completed_at) = {
        let runs = state.active_runs.read().await;
        if let Some(handle) = runs.get(&session_id) {
            let status = if matches!(handle.status, crate::RunStatus::Running)
                && completed_at_from_events.is_some()
            {
                "completed"
            } else {
                match &handle.status {
                    crate::RunStatus::Running => "running",
                    crate::RunStatus::Completed => "completed",
                    crate::RunStatus::Failed(_) => "failed",
                }
            };
            let completed_at = handle.completed_at.or(completed_at_from_events);
            (status, Some(handle.started_at), completed_at)
        } else {
            if let Some(completed_at) = completed_at_from_events {
                ("completed", None, Some(completed_at))
            } else {
                ("idle", None, None)
            }
        }
    };

    Ok(Json(StatusResponse {
        session_id,
        run_status,
        run_started_at,
        run_completed_at,
        event_count,
        latest_event,
    }))
}

fn is_terminal_event(payload: &lattice_core::EventPayload) -> bool {
    matches!(payload, lattice_core::EventPayload::FinalAnswer { .. })
}

fn to_session_sse_event(event: &lattice_core::Event) -> SseEvent {
    let data = serde_json::to_string(&EventResponse::from(event.clone()))
        .expect("event response serialization should not fail");
    SseEvent::default()
        .event("session_event")
        .id(event.event_id.to_string())
        .data(data)
}

fn done_event(session_id: SessionId) -> SseEvent {
    let data = serde_json::json!({
        "sessionId": session_id,
        "status": "completed",
    });
    SseEvent::default().event("done").data(data.to_string())
}

async fn session_has_terminal_event(
    state: &Arc<AppState>,
    session_id: SessionId,
) -> Result<bool, AppError> {
    let events = state
        .store
        .get_events(session_id, &lattice_core::EventFilter::default())
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;
    Ok(events
        .last()
        .is_some_and(|event| is_terminal_event(&event.payload)))
}
