//! Tool-related types and the ToolExecutor trait.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ToolError;
use crate::sandbox::ExecutionResult;

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
    async fn execute(&self, params: serde_json::Value) -> Result<ExecutionResult, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
