//! Core event types for Lattice's event-sourced architecture.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Event unique identifier.
pub type EventId = Uuid;

/// Session unique identifier.
pub type SessionId = Uuid;

/// Event timestamp.
pub type Timestamp = DateTime<Utc>;

/// Actor that produced an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Actor {
    /// System-level actor.
    System,
    /// LLM actor.
    LLM,
    /// Harness/control actor.
    Harness,
    /// Sandbox actor.
    Sandbox,
}

/// Event payload — all possible events in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EventPayload {
    /// Session was created.
    SessionCreated,
    /// User submitted a task.
    UserMessage { content: String },
    /// LLM is thinking.
    Thinking { reasoning: String },
    /// LLM decided to call a tool.
    ToolCallRequested {
        tool: String,
        params: serde_json::Value,
    },
    /// Tool call succeeded.
    ToolCallResult {
        stdout: String,
        stderr: String,
        exit_code: i32,
    },
    /// Tool call failed.
    ToolCallError { error: String },
    /// LLM gave a final answer.
    FinalAnswer { answer: String },
    /// Session state changed.
    StateChange { from: String, to: String },
}

/// An immutable, append-only event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique event identifier.
    pub event_id: EventId,
    /// Session this event belongs to.
    pub session_id: SessionId,
    /// When the event was produced.
    pub timestamp: Timestamp,
    /// Who produced this event.
    pub actor: Actor,
    /// Event data.
    pub payload: EventPayload,
    /// Parent event (for correlation).
    pub parent_event_id: Option<EventId>,
}


