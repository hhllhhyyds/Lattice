//! Sandbox types and the Sandbox trait.

use async_trait::async_trait;

use crate::error::SandboxError;

/// Sandbox execution result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionResult {
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Process exit code.
    pub exit_code: i32,
}

/// Sandbox — isolated tool execution environment.
///
/// A sandbox executes tool calls on behalf of the agent. It is responsible
/// for isolating tool execution from the host process and credentials.
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Execute a command in the sandbox.
    async fn execute(
        &self,
        command: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, SandboxError>;
}
