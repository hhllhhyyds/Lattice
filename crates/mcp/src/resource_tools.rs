use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{ExecutionContext, ExecutionResult, ToolDescription, ToolError, ToolExecutor};
use serde::Deserialize;
use serde_json::Value;

use crate::McpClientManager;

pub struct ListMcpResourcesTool {
    manager: Arc<McpClientManager>,
}

impl ListMcpResourcesTool {
    #[must_use]
    pub fn new(manager: Arc<McpClientManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolExecutor for ListMcpResourcesTool {
    fn description(&self) -> ToolDescription {
        ToolDescription {
            name: "list_mcp_resources".to_string(),
            description: "List MCP resources available from connected servers.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(
        &self,
        params: Value,
        _ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError> {
        validate_noop_params(params)?;

        let resources = self.manager.list_resources();
        let stdout = if resources.is_empty() {
            "(no MCP resources)".to_string()
        } else {
            resources
                .into_iter()
                .map(|item| {
                    let mut line = format!("{} {}", item.server_name, item.uri);
                    if !item.name.is_empty() {
                        line.push_str(&format!(" | {}", item.name));
                    }
                    if !item.description.is_empty() {
                        line.push_str(&format!(" | {}", item.description));
                    }
                    line
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(ExecutionResult {
            stdout,
            stderr: String::new(),
            exit_code: 0,
        })
    }
}

pub struct ReadMcpResourceTool {
    manager: Arc<McpClientManager>,
}

impl ReadMcpResourceTool {
    #[must_use]
    pub fn new(manager: Arc<McpClientManager>) -> Self {
        Self { manager }
    }
}

#[derive(Deserialize)]
struct ReadMcpResourceParams {
    server: String,
    uri: String,
}

#[async_trait]
impl ToolExecutor for ReadMcpResourceTool {
    fn description(&self) -> ToolDescription {
        ToolDescription {
            name: "read_mcp_resource".to_string(),
            description: "Read an MCP resource by server and URI.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "server": {
                        "type": "string",
                        "description": "MCP server name"
                    },
                    "uri": {
                        "type": "string",
                        "description": "Resource URI"
                    }
                },
                "required": ["server", "uri"]
            }),
        }
    }

    async fn execute(
        &self,
        params: Value,
        _ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError> {
        let parsed: ReadMcpResourceParams = serde_json::from_value(params).map_err(|err| {
            ToolError::InvalidParams(format!("invalid read_mcp_resource arguments: {err}"))
        })?;

        let stdout = self
            .manager
            .read_resource(&parsed.server, &parsed.uri)
            .await?;

        Ok(ExecutionResult {
            stdout,
            stderr: String::new(),
            exit_code: 0,
        })
    }
}

fn validate_noop_params(params: Value) -> Result<(), ToolError> {
    match params {
        Value::Null | Value::Object(_) => Ok(()),
        other => Err(ToolError::InvalidParams(format!(
            "expected object parameters, got {}",
            json_type_name(&other)
        ))),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
