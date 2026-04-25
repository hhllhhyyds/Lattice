//! Local subprocess sandbox implementation.
//!
//! Executes commands as local subprocesses via `tokio::process::Command`.
//! No isolation — use only for development and testing.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use lattice_core::{ExecutionResult, Sandbox, SandboxError};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::instrument;

/// Local sandbox that executes commands as subprocesses.
///
/// # Security
///
/// **This provides no isolation.** Commands run with the same OS user
/// privileges as the Lattice process. Do not use in untrusted environments.
#[derive(Debug, Clone)]
pub struct LocalSandbox {
    /// Optional working directory for command execution.
    work_dir: Option<PathBuf>,
    /// Execution timeout.
    timeout: Duration,
}

impl LocalSandbox {
    /// Create a new LocalSandbox with default settings.
    ///
    /// - `work_dir`: None (inherits process working directory)
    /// - `timeout`: 30 seconds
    #[must_use]
    pub fn new() -> Self {
        Self {
            work_dir: None,
            timeout: Duration::from_secs(30),
        }
    }

    /// Create a new LocalSandbox with a custom timeout.
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            work_dir: None,
            timeout,
        }
    }

    /// Create a new LocalSandbox with a custom working directory.
    #[must_use]
    pub fn with_work_dir(work_dir: PathBuf) -> Self {
        Self {
            work_dir: Some(work_dir),
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
    /// Execute a command in the sandbox.
    ///
    /// The `params` JSON must contain a `command` field with the shell command
    /// to run. The command is wrapped in `sh -c`.
    #[instrument(skip(self))]
    async fn execute(
        &self,
        _tool: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, SandboxError> {
        let cmd_str = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                SandboxError::ExecutionFailed("missing 'command' in params".to_string())
            })?;

        let mut cmd = Command::new("sh");
        cmd.args(["-c", cmd_str]);

        if let Some(ref dir) = self.work_dir {
            cmd.current_dir(dir);
        }

        let result =
            timeout(self.timeout, cmd.output())
                .await
                .map_err(|_| SandboxError::Timeout {
                    timeout_secs: self.timeout.as_secs(),
                })?;

        match result {
            Ok(output) => Ok(ExecutionResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1),
            }),
            Err(e) => Err(SandboxError::ExecutionFailed(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_stdout() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute("bash", serde_json::json!({ "command": "echo hello" }))
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_failed_command_exit_code() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute("bash", serde_json::json!({ "command": "exit 42" }))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn test_timeout() {
        let sandbox = LocalSandbox::with_timeout(Duration::from_millis(100));
        let result = sandbox
            .execute("bash", serde_json::json!({ "command": "sleep 10" }))
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SandboxError::Timeout { .. }));
    }

    #[tokio::test]
    async fn test_stderr_capture() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute("bash", serde_json::json!({ "command": "echo error >&2" }))
            .await
            .unwrap();
        assert_eq!(result.stderr.trim(), "error");
        assert_eq!(result.exit_code, 0);
    }
}
