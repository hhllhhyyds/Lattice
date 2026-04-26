//! Bash tool — delegates command execution to a Sandbox.

use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{ExecutionResult, Sandbox, ToolDescription, ToolError, ToolExecutor};

/// Bash tool — delegates command execution to a Sandbox.
///
/// Expects params: `{ "command": "ls -la" }`
pub struct BashTool {
    sandbox: Arc<dyn Sandbox>,
}

impl BashTool {
    /// Create a new BashTool backed by the given Sandbox.
    #[must_use]
    pub fn new(sandbox: Arc<dyn Sandbox>) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl ToolExecutor for BashTool {
    fn description(&self) -> ToolDescription {
        ToolDescription {
            name: "bash".to_string(),
            description: "Execute a bash command in a sandboxed environment. \
                 Use this for running shell commands, scripts, and system operations."
                .to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ExecutionResult, ToolError> {
        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidParams(
                    "missing or invalid 'command' field (expected string)".to_string(),
                )
            })?;
        // Pass `command` as the tool name and params as-is for LocalSandbox
        // (which re-extracts `command` from params internally).
        self.sandbox
            .execute(command, params.clone())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }
}

#[cfg(all(test, feature = "bash"))]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use lattice_core::{ExecutionResult, Sandbox, SandboxError, ToolError, ToolExecutor};

    use crate::bash::BashTool;

    /// Mock Sandbox for testing BashTool.
    struct MockSandbox {
        result: Result<ExecutionResult, SandboxError>,
    }

    impl MockSandbox {
        fn new(result: Result<ExecutionResult, SandboxError>) -> Self {
            Self { result }
        }
    }

    #[async_trait]
    impl Sandbox for MockSandbox {
        async fn execute(
            &self,
            _command: &str,
            _params: serde_json::Value,
        ) -> Result<ExecutionResult, SandboxError> {
            self.result.clone()
        }
    }

    #[tokio::test]
    async fn bash_tool_executes_via_sandbox() {
        let sandbox: Arc<dyn Sandbox> = Arc::new(MockSandbox::new(Ok(ExecutionResult {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: 0,
        })));
        let tool = BashTool::new(sandbox);

        let result: ExecutionResult = tool
            .execute(serde_json::json!({ "command": "echo hello" }))
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn bash_tool_invalid_params_missing_command() {
        let sandbox: Arc<dyn Sandbox> = Arc::new(MockSandbox::new(Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        })));
        let tool = BashTool::new(sandbox);

        let result = tool.execute(serde_json::json!({})).await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidParams(_)));
        assert!(err.to_string().contains("command"));
    }

    #[tokio::test]
    async fn bash_tool_invalid_params_wrong_type() {
        let sandbox: Arc<dyn Sandbox> = Arc::new(MockSandbox::new(Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        })));
        let tool = BashTool::new(sandbox);

        let result = tool.execute(serde_json::json!({ "command": 123 })).await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidParams(_)));
    }

    #[tokio::test]
    async fn bash_tool_sandbox_error() {
        let sandbox: Arc<dyn Sandbox> = Arc::new(MockSandbox::new(Err(SandboxError::Timeout {
            timeout_secs: 5,
        })));
        let tool = BashTool::new(sandbox);

        let result = tool
            .execute(serde_json::json!({ "command": "sleep 10" }))
            .await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }
}
