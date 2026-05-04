//! Bash tool — delegates command execution to a Sandbox.

use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{
    ExecutionContext, ExecutionResult, Sandbox, ToolDescription, ToolError, ToolExecutor,
};

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
        #[cfg(unix)]
        {
            ToolDescription {
                name: "sh".to_string(),
                description: "Execute a Unix shell command in a sandboxed environment. \
                     Use Unix-specific commands like 'ls' (list files), 'cat' (read file), \
                     'echo' (print text), 'grep' (search text), 'find' (search files)."
                    .to_string(),
                parameters_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The Unix shell command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            }
        }

        #[cfg(windows)]
        {
            ToolDescription {
                name: "cmd".to_string(),
                description: "Execute a Windows cmd.exe command in a sandboxed environment. \
                     Use Windows-specific commands like 'dir' (list files), 'type' (read file), \
                     'echo' (print text), 'findstr' (search text), 'where' (find files)."
                    .to_string(),
                parameters_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The Windows cmd.exe command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            }
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError> {
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
    use lattice_core::{
        Actor, ChildSessionInfo, Event, EventFilter, EventPayload, ExecutionContext,
        ExecutionResult, Sandbox, SandboxError, SessionId, StoreError, ToolError, ToolExecutor,
    };

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

    struct MockStore;

    #[async_trait]
    impl lattice_core::SessionStore for MockStore {
        async fn create_session(&self) -> Result<SessionId, StoreError> {
            Ok(SessionId::new_v4())
        }

        async fn delete_session(&self, _session_id: SessionId) -> Result<(), StoreError> {
            Ok(())
        }

        async fn append_event(
            &self,
            _session_id: SessionId,
            _payload: EventPayload,
            _actor: Actor,
            _parent_event_id: Option<lattice_core::EventId>,
        ) -> Result<lattice_core::EventId, StoreError> {
            Ok(lattice_core::EventId::new_v4())
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
        ) -> Result<(SessionId, Arc<dyn lattice_core::SessionStore>), StoreError> {
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
        ) -> Result<Option<lattice_core::EventId>, StoreError> {
            Ok(None)
        }
    }

    fn test_ctx() -> ExecutionContext {
        ExecutionContext {
            session_id: SessionId::new_v4(),
            trigger_event_id: lattice_core::EventId::new_v4(),
            store: Arc::new(MockStore),
            depth: 0,
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
            .execute(serde_json::json!({ "command": "echo hello" }), &test_ctx())
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

        let result = tool.execute(serde_json::json!({}), &test_ctx()).await;
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

        let result = tool
            .execute(serde_json::json!({ "command": 123 }), &test_ctx())
            .await;
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
            .execute(serde_json::json!({ "command": "sleep 10" }), &test_ctx())
            .await;
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }

    // Platform-aware tool description tests
    #[test]
    #[cfg(unix)]
    fn test_tool_name_on_unix() {
        let sandbox = Arc::new(MockSandbox::new(Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        })));
        let tool = BashTool::new(sandbox);
        let desc = tool.description();

        assert_eq!(desc.name, "sh", "Tool name should be 'sh' on Unix");
        assert!(
            desc.description.contains("Unix") || desc.description.contains("shell"),
            "Description should mention Unix or shell"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_tool_name_on_windows() {
        let sandbox = Arc::new(MockSandbox::new(Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        })));
        let tool = BashTool::new(sandbox);
        let desc = tool.description();

        assert_eq!(desc.name, "cmd", "Tool name should be 'cmd' on Windows");
        assert!(
            desc.description.contains("Windows") || desc.description.contains("cmd"),
            "Description should mention Windows or cmd"
        );
    }

    #[test]
    fn test_description_includes_platform_examples() {
        let sandbox = Arc::new(MockSandbox::new(Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        })));
        let tool = BashTool::new(sandbox);
        let desc = tool.description();

        // Description should include command examples
        #[cfg(unix)]
        {
            assert!(
                desc.description.contains("ls")
                    || desc.description.contains("cat")
                    || desc.description.contains("grep"),
                "Unix description should include command examples like ls, cat, or grep"
            );
        }

        #[cfg(windows)]
        {
            assert!(
                desc.description.contains("dir")
                    || desc.description.contains("type")
                    || desc.description.contains("findstr"),
                "Windows description should include command examples like dir, type, or findstr"
            );
        }
    }
}
