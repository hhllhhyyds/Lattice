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

/// Filter for querying events.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Optional actor filter.
    pub actor: Option<Actor>,
    /// Optional payload type filter.
    pub payload_type: Option<&'static str>,
}

/// LLM decision types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Decision {
    /// LLM is thinking — continue loop.
    Thinking { reasoning: String },
    /// LLM wants to call a tool.
    ToolCall {
        tool: String,
        params: serde_json::Value,
    },
    /// LLM is done.
    FinalAnswer { answer: String },
}

/// Tool description injected to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescription {
    /// Tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for parameters.
    pub parameters_schema: serde_json::Value,
}

/// Sandbox execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Process exit code.
    pub exit_code: i32,
}
