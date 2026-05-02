use std::collections::HashMap;

use rmcp::{
    model::{CallToolRequestParams, ErrorCode, Tool},
    service::{RoleClient, RunningService, ServiceError},
    transport::TokioChildProcess,
    ServiceExt,
};
use serde_json::Value;
use tokio::process::Command;
use tracing::warn;

use crate::types::{
    McpConnectionStatus, McpResourceInfo, McpServerConfig, McpStdioServerConfig, McpToolInfo,
};
use lattice_core::ToolError;

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolCallOutput {
    pub output: String,
    pub structured_content: Option<Value>,
}

pub struct McpClientManager {
    server_configs: HashMap<String, McpServerConfig>,
    statuses: HashMap<String, McpConnectionStatus>,
    sessions: HashMap<String, RunningService<RoleClient, ()>>,
}

impl McpClientManager {
    #[must_use]
    pub fn new(server_configs: HashMap<String, McpServerConfig>) -> Self {
        let statuses = server_configs
            .iter()
            .map(|(name, config)| {
                (
                    name.clone(),
                    McpConnectionStatus::pending(name.clone(), config.transport()),
                )
            })
            .collect();

        Self {
            server_configs,
            statuses,
            sessions: HashMap::new(),
        }
    }

    #[must_use]
    pub fn list_statuses(&self) -> Vec<McpConnectionStatus> {
        let mut statuses: Vec<_> = self.statuses.values().cloned().collect();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));
        statuses
    }

    #[must_use]
    pub fn list_tools(&self) -> Vec<McpToolInfo> {
        self.list_statuses()
            .into_iter()
            .flat_map(|status| status.tools)
            .collect()
    }

    #[must_use]
    pub fn list_resources(&self) -> Vec<McpResourceInfo> {
        self.list_statuses()
            .into_iter()
            .flat_map(|status| status.resources)
            .collect()
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<McpToolCallOutput, ToolError> {
        let session = self.sessions.get(server_name).ok_or_else(|| {
            ToolError::NotFound(format!("MCP server not connected: {server_name}"))
        })?;

        let request = match arguments {
            Value::Null => CallToolRequestParams::new(tool_name.to_string()),
            Value::Object(map) => {
                CallToolRequestParams::new(tool_name.to_string()).with_arguments(map)
            }
            other => {
                return Err(ToolError::InvalidParams(format!(
                    "MCP tool arguments must be a JSON object, got {}",
                    json_type_name(&other)
                )));
            }
        };

        let result = session.call_tool(request).await.map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "MCP tool call failed on server '{server_name}' for '{tool_name}': {err}"
            ))
        })?;

        let output = render_tool_result(&result);
        if result.is_error == Some(true) {
            return Err(ToolError::ExecutionFailed(output));
        }

        Ok(McpToolCallOutput {
            output,
            structured_content: result.structured_content,
        })
    }

    pub async fn connect_all(&mut self) {
        let names: Vec<String> = self.server_configs.keys().cloned().collect();
        for name in names {
            self.connect_one(&name).await;
        }
    }

    pub async fn reconnect_all(&mut self) {
        self.close().await;
        self.statuses = self
            .server_configs
            .iter()
            .map(|(name, config)| {
                (
                    name.clone(),
                    McpConnectionStatus::pending(name.clone(), config.transport()),
                )
            })
            .collect();
        self.connect_all().await;
    }

    pub async fn close(&mut self) {
        for session in self.sessions.values_mut() {
            if let Err(err) = session.close().await {
                warn!(?err, "failed to close MCP session cleanly");
            }
        }
        self.sessions.clear();

        for (name, config) in &self.server_configs {
            self.statuses.insert(
                name.clone(),
                McpConnectionStatus::pending(name.clone(), config.transport()),
            );
        }
    }

    async fn connect_one(&mut self, name: &str) {
        let Some(config) = self.server_configs.get(name).cloned() else {
            return;
        };

        match config {
            McpServerConfig::Stdio(stdio) => self.connect_stdio(name, stdio).await,
            unsupported => {
                self.statuses.insert(
                    name.to_string(),
                    McpConnectionStatus::failed(
                        name.to_string(),
                        unsupported.transport(),
                        format!(
                            "unsupported MCP transport in current build: {}",
                            unsupported.transport()
                        ),
                    ),
                );
            }
        }
    }

    async fn connect_stdio(&mut self, name: &str, config: McpStdioServerConfig) {
        let mut command = Command::new(&config.command);
        command.args(&config.args);
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }
        if let Some(env) = &config.env {
            command.envs(env);
        }

        let transport = match TokioChildProcess::new(command) {
            Ok(transport) => transport,
            Err(err) => {
                self.statuses.insert(
                    name.to_string(),
                    McpConnectionStatus::failed(name.to_string(), "stdio", err.to_string()),
                );
                return;
            }
        };

        let client = match ().serve(transport).await {
            Ok(client) => client,
            Err(err) => {
                self.statuses.insert(
                    name.to_string(),
                    McpConnectionStatus::failed(name.to_string(), "stdio", err.to_string()),
                );
                return;
            }
        };

        let tools = match client.list_all_tools().await {
            Ok(tools) => tools,
            Err(err) => {
                self.statuses.insert(
                    name.to_string(),
                    McpConnectionStatus::failed(name.to_string(), "stdio", err.to_string()),
                );
                return;
            }
        };

        let resources = match client.list_all_resources().await {
            Ok(resources) => resources,
            Err(err) if is_method_not_found(&err) => Vec::new(),
            Err(err) => {
                self.statuses.insert(
                    name.to_string(),
                    McpConnectionStatus::failed(name.to_string(), "stdio", err.to_string()),
                );
                return;
            }
        };

        self.statuses.insert(
            name.to_string(),
            McpConnectionStatus::connected(
                name.to_string(),
                "stdio",
                tools
                    .into_iter()
                    .map(|tool| to_tool_info(name, tool))
                    .collect(),
                resources
                    .into_iter()
                    .map(|resource| McpResourceInfo {
                        server_name: name.to_string(),
                        name: resource.raw.name,
                        uri: resource.raw.uri,
                        description: resource.raw.description.unwrap_or_default(),
                    })
                    .collect(),
            ),
        );
        self.sessions.insert(name.to_string(), client);
    }
}

fn to_tool_info(server_name: &str, tool: Tool) -> McpToolInfo {
    McpToolInfo {
        server_name: server_name.to_string(),
        name: tool.name.into_owned(),
        description: tool.description.map(|d| d.into_owned()).unwrap_or_default(),
        input_schema: tool.input_schema.as_ref().clone(),
    }
}

fn is_method_not_found(err: &ServiceError) -> bool {
    matches!(err, ServiceError::McpError(error) if error.code == ErrorCode::METHOD_NOT_FOUND)
}

fn render_tool_result(result: &rmcp::model::CallToolResult) -> String {
    let text_parts: Vec<String> = result
        .content
        .iter()
        .filter_map(|content| content.raw.as_text().map(|text| text.text.clone()))
        .collect();

    if !text_parts.is_empty() {
        return text_parts.join("\n");
    }

    if let Some(structured) = &result.structured_content {
        return serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string());
    }

    String::new()
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
