use std::collections::HashMap;

use rmcp::{
    model::{ErrorCode, Tool},
    service::{RoleClient, RunningService, ServiceError},
    transport::TokioChildProcess,
    ServiceExt,
};
use tokio::process::Command;
use tracing::warn;

use crate::types::{
    McpConnectionStatus, McpResourceInfo, McpServerConfig, McpStdioServerConfig, McpToolInfo,
};

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
