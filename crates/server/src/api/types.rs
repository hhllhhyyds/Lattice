//! Request and response DTOs for the Session API.

use chrono::{DateTime, Utc};
use lattice_core::{Actor, Event, EventId, SessionId};
use serde::{Deserialize, Serialize};

/// Optional metadata when creating a session.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    /// Optional session name.
    pub metadata: Option<SessionMetadata>,
}

/// Session metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    /// Human-readable name for the session.
    pub name: Option<String>,
    /// Arbitrary tags.
    pub tags: Vec<String>,
}

/// Summary info for a session (returned in list responses).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    /// Unique session identifier.
    pub session_id: SessionId,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// Current session status.
    pub status: SessionStatus,
    /// Total number of events in the session.
    pub event_count: usize,
    /// Optional run details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_info: Option<RunInfo>,
    /// Latest event id, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_event_id: Option<EventId>,
}

/// Session lifecycle status.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Session created but run not yet started.
    Created,
    /// Run is currently executing.
    Running,
    /// Run completed successfully.
    Completed,
    /// Run failed.
    Failed,
}

/// Run details included in a session detail response.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// Current run status.
    pub status: RunStatus,
}

/// Status of an active run.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    /// Run is currently executing.
    Running,
    /// Run completed successfully.
    Completed,
    /// Run failed with an error.
    Failed,
}

/// Response for listing sessions.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListResponse {
    /// All known sessions.
    pub sessions: Vec<SessionResponse>,
}

/// A single event in event list responses.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventResponse {
    /// Unique event identifier.
    pub event_id: EventId,
    /// Session this event belongs to.
    pub session_id: SessionId,
    /// When the event was produced.
    pub timestamp: DateTime<Utc>,
    /// Who produced this event.
    pub actor: Actor,
    /// Event payload data.
    pub payload: lattice_core::EventPayload,
    /// Parent event for correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<EventId>,
}

impl From<Event> for EventResponse {
    fn from(event: Event) -> Self {
        Self {
            event_id: event.event_id,
            session_id: event.session_id,
            timestamp: event.timestamp,
            actor: event.actor,
            payload: event.payload,
            parent_event_id: event.parent_event_id,
        }
    }
}

/// Response for listing events.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventListResponse {
    /// Matching events.
    pub events: Vec<EventResponse>,
    /// Whether more events exist beyond the returned set.
    pub has_more: bool,
}
