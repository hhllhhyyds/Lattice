//! Local subprocess sandbox implementation.

use std::time::Duration;

use async_trait::async_trait;
use lattice_core::{ExecutionResult, Sandbox};
use tokio::process::Command;
use tracing::instrument;

/// Local sandbox that executes commands as subprocesses.
///
/// NOTE: This provides no isolation — use only for development and testing.
pub struct LocalSandbox {
    #[allow(dead_code)]
    timeout: Duration,
}

impl LocalSandbox {
    #[must_use]
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

impl Default for LocalSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Sandbox for LocalSandbox {
    #[instrument(skip(self))]
    async fn execute(
        &self,
        command: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, lattice_core::SandboxError> {
        let cmd_str = params
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or(command);

        let output = Command::new("sh")
            .args(["-c", cmd_str])
            .output()
            .await
            .map_err(|e| lattice_core::SandboxError(e.to_string()))?;

        Ok(ExecutionResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_echo() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute("echo", serde_json::json!({ "command": "echo hello" }))
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }
}
