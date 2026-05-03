use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{ExecutionResult, ToolDescription, ToolError, ToolExecutor};
use serde_json::Value;

use crate::{McpClientManager, McpToolInfo};

pub fn mcp_tool_name(server_name: &str, tool_name: &str) -> String {
    format!(
        "mcp__{}__{}",
        sanitize_tool_segment(server_name),
        sanitize_tool_segment(tool_name)
    )
}

pub struct McpToolAdapter {
    manager: Arc<McpClientManager>,
    tool_info: McpToolInfo,
}

impl McpToolAdapter {
    #[must_use]
    pub fn new(manager: Arc<McpClientManager>, tool_info: McpToolInfo) -> Self {
        Self { manager, tool_info }
    }

    #[must_use]
    pub fn from_manager(manager: Arc<McpClientManager>) -> Vec<Self> {
        manager
            .list_tools()
            .into_iter()
            .map(|tool_info| Self::new(manager.clone(), tool_info))
            .collect()
    }
}

#[async_trait]
impl ToolExecutor for McpToolAdapter {
    fn description(&self) -> ToolDescription {
        ToolDescription {
            name: mcp_tool_name(&self.tool_info.server_name, &self.tool_info.name),
            description: if self.tool_info.description.is_empty() {
                format!("MCP tool {}", self.tool_info.name)
            } else {
                self.tool_info.description.clone()
            },
            parameters_schema: Value::Object(self.tool_info.input_schema.clone()),
        }
    }

    async fn execute(&self, params: Value) -> Result<ExecutionResult, ToolError> {
        let output = self
            .manager
            .call_tool(&self.tool_info.server_name, &self.tool_info.name, params)
            .await?;

        Ok(ExecutionResult {
            stdout: output.output,
            stderr: String::new(),
            exit_code: 0,
        })
    }
}

fn sanitize_tool_segment(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len().max(4));
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }

    if sanitized.is_empty() {
        return "tool".to_string();
    }

    if sanitized
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
    {
        sanitized
    } else {
        format!("mcp_{sanitized}")
    }
}

#[cfg(test)]
mod tests {
    use super::mcp_tool_name;

    #[test]
    fn tool_name_is_sanitized() {
        assert_eq!(
            mcp_tool_name("fixture server", "hello/world"),
            "mcp__fixture_server__hello_world"
        );
        assert_eq!(mcp_tool_name("123", "!"), "mcp__mcp_123__mcp__");
    }
}
