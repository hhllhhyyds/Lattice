//! Tool set — registry of available tools.

use std::collections::HashMap;
use std::sync::Arc;

use lattice_core::{Sandbox, ToolDescription, ToolError, ToolExecutor};
use tracing::instrument;

/// A collection of tools available to the agent.
///
/// ToolSet serves two roles:
/// 1. Provide tool descriptions to the LLM (via `descriptions()`)
/// 2. Route tool calls to the correct executor (via `execute()`)
pub struct ToolSet {
    tools: HashMap<String, Box<dyn ToolExecutor>>,
}

impl Default for ToolSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolSet {
    /// Create an empty ToolSet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Build a ToolSet with all default tools enabled by compiled features.
    ///
    /// Currently includes:
    /// - `BashTool` (if the `bash` feature is enabled)
    #[must_use]
    pub fn with_defaults(sandbox: Arc<dyn Sandbox>) -> Self {
        let mut set = Self::new();
        #[cfg(feature = "bash")]
        set.register(crate::bash::BashTool::new(sandbox))
            .expect("bash tool registration should not fail");
        set
    }

    /// Register a tool. Returns error if a tool with the same name already exists.
    pub fn register(&mut self, tool: impl ToolExecutor + 'static) -> Result<(), ToolError> {
        let name = tool.description().name.clone();
        if self.tools.contains_key(&name) {
            return Err(ToolError::Other(format!(
                "tool '{name}' is already registered"
            )));
        }
        self.tools.insert(name, Box::new(tool));
        Ok(())
    }

    /// List all tool descriptions (passed to LLMClient::decide).
    #[must_use]
    pub fn descriptions(&self) -> Vec<ToolDescription> {
        self.tools.values().map(|t| t.description()).collect()
    }

    /// Look up and execute a tool by name.
    #[instrument(skip(self))]
    pub async fn execute(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<lattice_core::ExecutionResult, ToolError> {
        let executor = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        executor.execute(params).await
    }

    /// Check if a tool is registered.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use lattice_core::{ExecutionResult, ToolDescription, ToolError, ToolExecutor};

    use crate::ToolSet;

    struct MockTool {
        name: String,
        result: Result<ExecutionResult, ToolError>,
    }

    impl MockTool {
        fn new(name: &str, result: Result<ExecutionResult, ToolError>) -> Self {
            Self {
                name: name.to_string(),
                result,
            }
        }
    }

    #[async_trait]
    impl ToolExecutor for MockTool {
        fn description(&self) -> ToolDescription {
            ToolDescription {
                name: self.name.clone(),
                description: "Mock tool".to_string(),
                parameters_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            }
        }

        async fn execute(&self, _params: serde_json::Value) -> Result<ExecutionResult, ToolError> {
            self.result.clone()
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut set = ToolSet::new();
        let tool = MockTool::new(
            "test",
            Ok(ExecutionResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            }),
        );
        set.register(tool).unwrap();
        assert!(set.contains("test"));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn duplicate_name_returns_error() {
        let mut set = ToolSet::new();
        let tool1 = MockTool::new(
            "dup",
            Ok(ExecutionResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            }),
        );
        let tool2 = MockTool::new(
            "dup",
            Ok(ExecutionResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            }),
        );
        set.register(tool1).unwrap();
        let err = set.register(tool2).unwrap_err();
        assert!(matches!(err, ToolError::Other(_)));
        assert!(err.to_string().contains("dup"));
    }

    #[test]
    fn execute_unknown_tool_returns_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let set = ToolSet::new();
        let result = rt.block_on(set.execute("nonexistent", serde_json::json!({})));
        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[tokio::test]
    async fn bash_tool_invalid_params() {
        let set = ToolSet::new();
        // No tools registered — execute returns NotFound.
        let result = set
            .execute("bash", serde_json::json!({ "command": 123 }))
            .await;
        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[test]
    fn descriptions_returns_all_tools() {
        let mut set = ToolSet::new();
        set.register(MockTool::new(
            "tool_a",
            Ok(ExecutionResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            }),
        ))
        .unwrap();
        set.register(MockTool::new(
            "tool_b",
            Ok(ExecutionResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            }),
        ))
        .unwrap();

        let descs = set.descriptions();
        assert_eq!(descs.len(), 2);
        let names: Vec<_> = descs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[tokio::test]
    async fn test_execute_success_returns_result() {
        let result = ExecutionResult {
            stdout: "hello world".to_string(),
            stderr: "err output".to_string(),
            exit_code: 42,
        };
        let tool = MockTool::new("success", Ok(result.clone()));
        let mut set = ToolSet::new();
        set.register(tool).unwrap();

        let got = set.execute("success", serde_json::json!({})).await.unwrap();
        assert_eq!(got.stdout, "hello world");
        assert_eq!(got.stderr, "err output");
        assert_eq!(got.exit_code, 42);
    }

    #[cfg(feature = "bash")]
    #[test]
    fn test_with_defaults_contains_bash() {
        use lattice_core::Sandbox;
        use std::sync::Arc;

        struct MockSandbox;
        #[async_trait::async_trait]
        impl Sandbox for MockSandbox {
            async fn execute(
                &self,
                _command: &str,
                _params: serde_json::Value,
            ) -> Result<lattice_core::ExecutionResult, lattice_core::SandboxError> {
                unreachable!()
            }
        }

        let set = crate::ToolSet::with_defaults(Arc::new(MockSandbox));

        // Tool name is platform-specific: "sh" on Unix, "cmd" on Windows
        #[cfg(unix)]
        assert!(set.contains("sh"), "ToolSet should contain 'sh' on Unix");

        #[cfg(windows)]
        assert!(
            set.contains("cmd"),
            "ToolSet should contain 'cmd' on Windows"
        );

        assert_eq!(set.len(), 1);
    }
}
