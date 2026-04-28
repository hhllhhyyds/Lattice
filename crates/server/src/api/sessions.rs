//! Session API handlers.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use lattice_core::{Actor, EventFilter, EventId, SessionId};
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

fn default_limit() -> usize {
    100
}

/// Registers all /v1 session routes.
pub fn v1_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sessions", post(create_session).get(list_sessions))
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}/events", get(get_events))
        .route(
            "/sessions/{id}/messages",
            post(submit_message).get(get_messages),
        )
        .route("/sessions/{id}/status", get(get_status))
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
            metadata,
        });
    }

    let response = SessionResponse {
        session_id,
        created_at,
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
        Some(handle) => match handle.status {
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
            let status = match handle.status {
                crate::RunStatus::Running => SessionStatus::Running,
                crate::RunStatus::Completed => SessionStatus::Completed,
                crate::RunStatus::Failed(_) => SessionStatus::Failed,
            };
            let run_status = match handle.status {
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
        if runs.contains_key(&session_id) {
            return Err(AppError::Conflict(
                "Session already has a running task".into(),
            ));
        }
    }

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

    {
        let mut runs = state.active_runs.write().await;
        let join_handle = tokio::spawn(async {
            // TODO: Actually run ControlLoop here.
            // For now, just sleep briefly to simulate work.
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        });

        runs.insert(
            session_id,
            crate::RunHandle {
                session_id,
                status: crate::RunStatus::Running,
                started_at,
                abort_handle: join_handle.abort_handle(),
            },
        );
    }

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

    // Check run status.
    let (run_status, run_started_at, run_completed_at) = {
        let runs = state.active_runs.read().await;
        if let Some(handle) = runs.get(&session_id) {
            let status = match handle.status {
                crate::RunStatus::Running => "running",
                crate::RunStatus::Completed => "completed",
                crate::RunStatus::Failed(_) => "failed",
            };
            let completed_at = match handle.status {
                crate::RunStatus::Completed | crate::RunStatus::Failed(_) => {
                    Some(chrono::Utc::now())
                }
                _ => None,
            };
            (status, Some(handle.started_at), completed_at)
        } else {
            ("idle", None, None)
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
