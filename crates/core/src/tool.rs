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
