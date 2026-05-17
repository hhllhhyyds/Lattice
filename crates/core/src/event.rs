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

/// Structured category for tool execution failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorKind {
    /// Tool name was not found in the registry.
    NotFound,
    /// Tool parameters failed validation.
    InvalidParams,
    /// Tool execution failed after starting.
    ExecutionFailed,
    /// Skill recursion exceeded the configured depth limit.
    MaxDepthExceeded,
    /// Tool execution exceeded a timeout.
    Timeout,
    /// Fallback for uncategorized errors.
    Other,
}

impl ToolErrorKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::InvalidParams => "invalid_params",
            Self::ExecutionFailed => "execution_failed",
            Self::MaxDepthExceeded => "max_depth_exceeded",
            Self::Timeout => "timeout",
            Self::Other => "other",
        }
    }
}

/// Event payload - all possible events in the system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EventPayload {
    /// Session was created.
    SessionCreated,
    /// User submitted a task.
    UserMessage { content: String },
    /// LLM is thinking.
    Thinking {
        reasoning: String,
        /// Opaque signature required by some providers for round-trip verification.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
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
    ToolCallError {
        error: String,
        error_kind: ToolErrorKind,
    },
    /// A skill was invoked - recorded in the parent session.
    SkillInvoked {
        skill_name: String,
        child_session_id: SessionId,
    },
    /// A skill completed - recorded in the parent session.
    SkillCompleted {
        skill_name: String,
        child_session_id: SessionId,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actor_serde_roundtrip() {
        for actor in [Actor::System, Actor::LLM, Actor::Harness, Actor::Sandbox] {
            let json = serde_json::to_string(&actor).unwrap();
            let parsed: Actor = serde_json::from_str(&json).unwrap();
            assert_eq!(actor, parsed);
        }
    }

    #[test]
    fn test_event_payload_serde_roundtrip() {
        let payloads = vec![
            EventPayload::SessionCreated,
            EventPayload::UserMessage {
                content: "hello world".to_string(),
            },
            EventPayload::Thinking {
                reasoning: "let me think".to_string(),
                signature: None,
            },
            EventPayload::ToolCallRequested {
                tool: "bash".to_string(),
                params: serde_json::json!({ "command": "echo hi" }),
            },
            EventPayload::ToolCallResult {
                stdout: "hi".to_string(),
                stderr: String::new(),
                exit_code: 0,
            },
            EventPayload::ToolCallError {
                error: "not found".to_string(),
                error_kind: ToolErrorKind::NotFound,
            },
            EventPayload::SkillInvoked {
                skill_name: "web-research".to_string(),
                child_session_id: SessionId::new_v4(),
            },
            EventPayload::SkillCompleted {
                skill_name: "web-research".to_string(),
                child_session_id: SessionId::new_v4(),
            },
            EventPayload::FinalAnswer {
                answer: "the answer".to_string(),
            },
            EventPayload::StateChange {
                from: "idle".to_string(),
                to: "running".to_string(),
            },
        ];
        for payload in payloads {
            let json = serde_json::to_string(&payload).unwrap();
            let parsed: EventPayload = serde_json::from_str(&json).unwrap();
            assert_eq!(payload, parsed);
        }
    }

    #[test]
    fn test_event_serde_roundtrip() {
        let event = Event {
            event_id: EventId::new_v4(),
            session_id: SessionId::new_v4(),
            timestamp: chrono::Utc::now(),
            actor: Actor::LLM,
            payload: EventPayload::UserMessage {
                content: "test message".to_string(),
            },
            parent_event_id: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event.event_id, parsed.event_id);
        assert_eq!(event.session_id, parsed.session_id);
        assert_eq!(event.actor, parsed.actor);
        assert_eq!(event.payload, parsed.payload);
        assert_eq!(event.parent_event_id, parsed.parent_event_id);
    }

    #[test]
    fn test_event_payload_tagged_format() {
        let payload = EventPayload::UserMessage {
            content: "hello".to_string(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""type":"userMessage""#));
        assert!(json.contains(r#""content":"hello""#));
    }

    #[test]
    fn test_tool_call_requested_tagged_format() {
        let payload = EventPayload::ToolCallRequested {
            tool: "bash".to_string(),
            params: serde_json::json!({ "command": "ls" }),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""type":"toolCallRequested""#));
        assert!(json.contains(r#""tool":"bash""#));
    }

    #[test]
    fn test_tool_call_result_tagged_format() {
        let payload = EventPayload::ToolCallResult {
            stdout: "output".to_string(),
            stderr: "err".to_string(),
            exit_code: 0,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""type":"toolCallResult""#));
        assert!(json.contains(r#""stdout":"output""#));
        assert!(json.contains(r#""exit_code":0"#));
    }

    #[test]
    fn test_tool_call_error_kind_tagged_format() {
        let payload = EventPayload::ToolCallError {
            error: "missing command".to_string(),
            error_kind: ToolErrorKind::InvalidParams,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""type":"toolCallError""#));
        assert!(json.contains(r#""error":"missing command""#));
        assert!(json.contains(r#""error_kind":"invalid_params""#));
    }

    #[test]
    fn test_final_answer_tagged_format() {
        let payload = EventPayload::FinalAnswer {
            answer: "42".to_string(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""type":"finalAnswer""#));
        assert!(json.contains(r#""answer":"42""#));
    }

    #[test]
    fn test_skill_invoked_serde_roundtrip() {
        let payload = EventPayload::SkillInvoked {
            skill_name: "web-research".to_string(),
            child_session_id: SessionId::new_v4(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let parsed: EventPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload, parsed);
        assert!(json.contains(r#""type":"skillInvoked""#));
    }

    #[test]
    fn test_skill_completed_serde_roundtrip() {
        let payload = EventPayload::SkillCompleted {
            skill_name: "web-research".to_string(),
            child_session_id: SessionId::new_v4(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let parsed: EventPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload, parsed);
        assert!(json.contains(r#""type":"skillCompleted""#));
    }
}
