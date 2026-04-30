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
    /// Optional session metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SessionMetadata>,
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

// --- Task 16: Agent Run API types ---

/// Request to submit a message and trigger agent execution.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitMessageRequest {
    /// User message content.
    #[serde(default)]
    pub content: String,
    /// Optional LLM provider (e.g., "anthropic", "openai").
    pub provider: Option<String>,
    /// Optional model name (e.g., "gpt-4o", "claude-3-5-sonnet-20241022").
    pub model: Option<String>,
    /// Optional system prompt override.
    pub system_prompt: Option<String>,
    /// Optional maximum number of ControlLoop iterations.
    pub max_iterations: Option<usize>,
}

/// Request to trigger agent execution for an existing session.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    /// Optional LLM provider (e.g., "anthropic", "openai").
    pub provider: Option<String>,
    /// Optional model name.
    pub model: Option<String>,
    /// Optional system prompt override.
    pub system_prompt: Option<String>,
    /// Optional maximum number of ControlLoop iterations.
    pub max_iterations: Option<usize>,
}

/// Response after submitting a message (202 Accepted).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitMessageResponse {
    /// Session ID for this run.
    pub session_id: SessionId,
    /// Unique run identifier.
    pub run_id: String,
    /// Current run status (always "running" on submission).
    pub status: &'static str,
    /// Human-readable message.
    pub message: &'static str,
}

/// A message in the conversation (user or assistant).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Role: "user" or "assistant".
    pub role: &'static str,
    /// Message content.
    pub content: String,
    /// When the message was created.
    pub timestamp: DateTime<Utc>,
}

/// Response for GET /v1/sessions/:id/messages.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagesResponse {
    /// Conversation messages (UserMessage + FinalAnswer events).
    pub messages: Vec<Message>,
}

/// Response for GET /v1/sessions/:id/status.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    /// Session ID.
    pub session_id: SessionId,
    /// Current run status: "idle", "running", "completed", "failed".
    pub run_status: &'static str,
    /// When the run started (null if idle).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_started_at: Option<DateTime<Utc>>,
    /// When the run completed (null if not completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_completed_at: Option<DateTime<Utc>>,
    /// Total number of events in the session.
    pub event_count: usize,
    /// Latest event summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_event: Option<LatestEventInfo>,
}

/// Summary of the latest event in a session.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestEventInfo {
    /// Event ID.
    pub event_id: EventId,
    /// Actor who produced the event.
    pub actor: Actor,
    /// Payload type name (e.g., "sessionCreated", "toolCallRequested").
    pub payload_type: String,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
}
