//! Tool-related types and the ToolExecutor trait.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ToolError;
use crate::sandbox::ExecutionResult;
use crate::{EventId, SessionId, SessionStore};

/// Tool description injected to the LLM.
///
/// Describes a callable tool so the LLM can decide when and how to use it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescription {
    /// Tool name (must be unique within a session).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub parameters_schema: serde_json::Value,
}

/// Maximum allowed skill nesting depth.
pub const MAX_SKILL_DEPTH: u32 = 8;

/// Execution context passed to every tool invocation by the ControlLoop.
#[derive(Clone)]
pub struct ExecutionContext {
    /// The session this tool call belongs to.
    pub session_id: SessionId,
    /// The event id of the `ToolCallRequested` event that triggered this execution.
    pub trigger_event_id: EventId,
    /// The session store for reading and writing correlated events.
    pub store: Arc<dyn SessionStore>,
    /// Nesting depth: 0 for the top-level agent, 1 for direct skill children, etc.
    pub depth: u32,
}

impl fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("session_id", &self.session_id)
            .field("trigger_event_id", &self.trigger_event_id)
            .field("depth", &self.depth)
            .finish()
    }
}

/// A tool that can be executed by the agent.
///
/// Implementations can be in-process (file read, HTTP fetch) or delegate
/// to a Sandbox (bash, python). The ControlLoop treats all tools identically
/// through this trait.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Return the tool description for LLM consumption.
    fn description(&self) -> ToolDescription;

    /// Execute the tool with the given parameters.
    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, ChildSessionInfo, Event, EventFilter, EventPayload, SessionId, StoreError};
    use async_trait::async_trait;

    struct MockStore;

    #[async_trait]
    impl SessionStore for MockStore {
        async fn create_session(&self) -> Result<SessionId, StoreError> {
            Ok(SessionId::new_v4())
        }

        async fn append_event(
            &self,
            _session_id: SessionId,
            _payload: EventPayload,
            _actor: Actor,
            _parent_event_id: Option<crate::EventId>,
        ) -> Result<crate::EventId, StoreError> {
            Ok(crate::EventId::new_v4())
        }

        async fn get_events(
            &self,
            _session_id: SessionId,
            _filter: &EventFilter,
        ) -> Result<Vec<Event>, StoreError> {
            Ok(Vec::new())
        }

        async fn create_child_session(
            &self,
            _parent_session_id: SessionId,
            _skill_name: &str,
        ) -> Result<(SessionId, Arc<dyn SessionStore>), StoreError> {
            Ok((SessionId::new_v4(), Arc::new(MockStore)))
        }

        async fn child_sessions(
            &self,
            _parent_session_id: SessionId,
        ) -> Result<Vec<ChildSessionInfo>, StoreError> {
            Ok(Vec::new())
        }

        async fn latest_event_id(
            &self,
            _session_id: SessionId,
        ) -> Result<Option<crate::EventId>, StoreError> {
            Ok(None)
        }
    }

    #[test]
    fn test_tool_description_serde_roundtrip() {
        let desc = ToolDescription {
            name: "bash".to_string(),
            description: "Execute a bash command".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        };
        let json = serde_json::to_string(&desc).unwrap();
        let parsed: ToolDescription = serde_json::from_str(&json).unwrap();
        assert_eq!(desc.name, parsed.name);
        assert_eq!(desc.description, parsed.description);
        assert_eq!(desc.parameters_schema, parsed.parameters_schema);
    }

    #[test]
    fn test_tool_description_serde_format() {
        let desc = ToolDescription {
            name: "echo".to_string(),
            description: "Echo back the input".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "msg": { "type": "string" }
                },
                "required": []
            }),
        };
        let json = serde_json::to_string(&desc).unwrap();
        assert!(json.contains(r#""name":"echo""#));
        assert!(json.contains(r#""description":"Echo back"#));
        assert!(json.contains(r#""type":"object""#));
    }

    #[test]
    fn execution_context_clone() {
        let ctx = ExecutionContext {
            session_id: SessionId::new_v4(),
            trigger_event_id: crate::EventId::new_v4(),
            store: Arc::new(MockStore),
            depth: 3,
        };
        let cloned = ctx.clone();
        assert_eq!(cloned.depth, 3);
        assert_eq!(cloned.session_id, ctx.session_id);
    }

    #[test]
    fn max_skill_depth_value() {
        assert_eq!(MAX_SKILL_DEPTH, 8);
    }
}
