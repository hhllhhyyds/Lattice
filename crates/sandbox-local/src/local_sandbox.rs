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
use tracing::{debug, info, instrument};

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
    /// to run. The command is wrapped in the platform-specific shell:
    /// - Unix/Linux/macOS: `sh -c`
    /// - Windows: `cmd.exe /C`
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

        debug!("executing command: {}", cmd_str);

        // Platform-specific shell selection
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.args(["-c", cmd_str]);
            c
        };

        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/C", cmd_str]);
            c
        };

        if let Some(ref dir) = self.work_dir {
            cmd.current_dir(dir);
        }

        info!(
            "starting command execution with timeout of {} seconds",
            self.timeout.as_secs()
        );

        let result = timeout(self.timeout, cmd.output()).await.map_err(|_| {
            info!("command timed out after {} seconds", self.timeout.as_secs());
            SandboxError::Timeout {
                timeout_secs: self.timeout.as_secs(),
            }
        })?;

        match result {
            Ok(output) => {
                let exec_result = ExecutionResult {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code().unwrap_or(-1),
                };
                info!(
                    "command completed: exit_code={}, stdout_len={}, stderr_len={}",
                    exec_result.exit_code,
                    exec_result.stdout.len(),
                    exec_result.stderr.len()
                );
                Ok(exec_result)
            }
            Err(e) => {
                info!("command execution failed: {}", e);
                Err(SandboxError::ExecutionFailed(e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Platform-specific test commands
    #[cfg(unix)]
    mod platform_commands {
        pub const TOOL_NAME: &str = "sh";
        pub const ECHO_HELLO: &str = "echo hello";
        pub const EXIT_42: &str = "exit 42";
        pub const SLEEP_LONG: &str = "sleep 10";
        pub const ECHO_STDERR: &str = "echo error >&2";
    }

    #[cfg(windows)]
    mod platform_commands {
        pub const TOOL_NAME: &str = "cmd";
        pub const ECHO_HELLO: &str = "echo hello";
        pub const EXIT_42: &str = "exit 42";
        // Use ping as a sleep alternative on Windows (more reliable for timeout tests)
        // Note: ping output is ignored by the test, we only care about the timeout
        pub const SLEEP_LONG: &str = "ping -n 11 127.0.0.1";
        pub const ECHO_STDERR: &str = "echo error 1>&2";
    }

    use platform_commands::*;

    #[tokio::test]
    async fn test_echo_stdout() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute(TOOL_NAME, serde_json::json!({ "command": ECHO_HELLO }))
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_failed_command_exit_code() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute(TOOL_NAME, serde_json::json!({ "command": EXIT_42 }))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn test_timeout() {
        let sandbox = LocalSandbox::with_timeout(Duration::from_millis(100));
        let result = sandbox
            .execute(TOOL_NAME, serde_json::json!({ "command": SLEEP_LONG }))
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SandboxError::Timeout { .. }));
    }

    #[tokio::test]
    async fn test_stderr_capture() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute(TOOL_NAME, serde_json::json!({ "command": ECHO_STDERR }))
            .await
            .unwrap();
        assert_eq!(result.stderr.trim(), "error");
        assert_eq!(result.exit_code, 0);
    }

    // Cross-platform integration test: verify basic command execution works
    #[tokio::test]
    async fn test_cross_platform_basic_command() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute(TOOL_NAME, serde_json::json!({ "command": "echo test" }))
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "test");
        assert_eq!(result.exit_code, 0);
    }

    // Test platform-specific shell behavior
    #[tokio::test]
    #[cfg(windows)]
    async fn test_windows_uses_cmd() {
        let sandbox = LocalSandbox::new();
        // This command only works in cmd.exe, not in sh
        let result = sandbox
            .execute("cmd", serde_json::json!({ "command": "echo %OS%" }))
            .await
            .unwrap();
        // In cmd.exe, %OS% expands to "Windows_NT"
        // In sh, it would output literal "%OS%"
        assert!(
            result.stdout.contains("Windows_NT"),
            "Expected cmd.exe to expand %OS% to Windows_NT, got: {}",
            result.stdout
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_unix_uses_sh() {
        let sandbox = LocalSandbox::new();
        // This command only works in sh, not in cmd.exe
        let result = sandbox
            .execute("sh", serde_json::json!({ "command": "echo $SHELL" }))
            .await
            .unwrap();
        // In sh, $SHELL expands to the shell path
        // In cmd.exe, it would output literal "$SHELL"
        assert!(!result.stdout.trim().is_empty());
    }

    // Error handling tests
    #[tokio::test]
    async fn test_missing_command_parameter() {
        let sandbox = LocalSandbox::new();
        let result = sandbox.execute(TOOL_NAME, serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SandboxError::ExecutionFailed(_)
        ));
    }

    #[tokio::test]
    async fn test_invalid_command_type() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute(TOOL_NAME, serde_json::json!({ "command": 123 }))
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SandboxError::ExecutionFailed(_)
        ));
    }

    #[tokio::test]
    async fn test_nonexistent_command() {
        let sandbox = LocalSandbox::new();
        let result = sandbox
            .execute(
                TOOL_NAME,
                serde_json::json!({ "command": "nonexistent_command_xyz_12345" }),
            )
            .await;
        // Command doesn't exist, should return non-zero exit code or error
        match result {
            Ok(exec_result) => assert_ne!(exec_result.exit_code, 0),
            Err(_) => {} // May also return error directly
        }
    }

    // Working directory tests
    #[tokio::test]
    async fn test_with_work_dir() {
        use std::env;
        let temp_dir = env::temp_dir();
        let sandbox = LocalSandbox::with_work_dir(temp_dir.clone());

        #[cfg(unix)]
        let result = sandbox
            .execute("sh", serde_json::json!({ "command": "pwd" }))
            .await
            .unwrap();

        #[cfg(windows)]
        let result = sandbox
            .execute("cmd", serde_json::json!({ "command": "cd" }))
            .await
            .unwrap();

        // Just verify the command executed successfully
        // The exact output format may vary by platform
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_invalid_work_dir() {
        use std::path::PathBuf;
        let invalid_dir = PathBuf::from("/nonexistent/directory/xyz/abc");
        let sandbox = LocalSandbox::with_work_dir(invalid_dir);

        let result = sandbox
            .execute(TOOL_NAME, serde_json::json!({ "command": "echo test" }))
            .await;

        // Should return error because working directory doesn't exist
        assert!(result.is_err());
    }

    // Constructor tests
    #[test]
    fn test_default() {
        let sandbox = LocalSandbox::default();
        assert_eq!(sandbox.timeout, Duration::from_secs(30));
        assert!(sandbox.work_dir.is_none());
    }

    #[test]
    fn test_with_timeout_constructor() {
        let sandbox = LocalSandbox::with_timeout(Duration::from_secs(60));
        assert_eq!(sandbox.timeout, Duration::from_secs(60));
        assert!(sandbox.work_dir.is_none());
    }

    #[test]
    fn test_with_work_dir_constructor() {
        use std::path::PathBuf;
        let dir = PathBuf::from("/tmp");
        let sandbox = LocalSandbox::with_work_dir(dir.clone());
        assert_eq!(sandbox.work_dir, Some(dir));
        assert_eq!(sandbox.timeout, Duration::from_secs(30));
    }
}
