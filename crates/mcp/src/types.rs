use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    Stdio(McpStdioServerConfig),
    Http(McpHttpServerConfig),
    Ws(McpWebSocketServerConfig),
}

impl McpServerConfig {
    #[must_use]
    pub fn transport(&self) -> &'static str {
        match self {
            Self::Stdio(_) => "stdio",
            Self::Http(_) => "http",
            Self::Ws(_) => "ws",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpStdioServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpHttpServerConfig {
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpWebSocketServerConfig {
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct McpJsonConfig {
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpConnectionState {
    Pending,
    Connected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server_name: String,
    pub name: String,
    pub description: String,
    pub input_schema: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpResourceInfo {
    pub server_name: String,
    pub name: String,
    pub uri: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpConnectionStatus {
    pub name: String,
    pub state: McpConnectionState,
    pub detail: String,
    pub transport: String,
    pub tools: Vec<McpToolInfo>,
    pub resources: Vec<McpResourceInfo>,
}

impl McpConnectionStatus {
    #[must_use]
    pub fn pending(name: impl Into<String>, transport: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            state: McpConnectionState::Pending,
            detail: String::new(),
            transport: transport.into(),
            tools: Vec::new(),
            resources: Vec::new(),
        }
    }

    #[must_use]
    pub fn connected(
        name: impl Into<String>,
        transport: impl Into<String>,
        tools: Vec<McpToolInfo>,
        resources: Vec<McpResourceInfo>,
    ) -> Self {
        Self {
            name: name.into(),
            state: McpConnectionState::Connected,
            detail: String::new(),
            transport: transport.into(),
            tools,
            resources,
        }
    }

    #[must_use]
    pub fn failed(
        name: impl Into<String>,
        transport: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            state: McpConnectionState::Failed,
            detail: detail.into(),
            transport: transport.into(),
            tools: Vec::new(),
            resources: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_config_parses_tagged_server_configs() {
        let json = r#"{
          "mcpServers": {
            "fixture": {
              "type": "stdio",
              "command": "python",
              "args": ["server.py"]
            },
            "remote": {
              "type": "http",
              "url": "https://example.com/mcp",
              "headers": {
                "Authorization": "Bearer token"
              }
            }
          }
        }"#;

        let parsed: McpJsonConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.mcp_servers.len(), 2);
        assert!(matches!(
            parsed.mcp_servers.get("fixture"),
            Some(McpServerConfig::Stdio(McpStdioServerConfig { command, .. })) if command == "python"
        ));
        assert!(matches!(
            parsed.mcp_servers.get("remote"),
            Some(McpServerConfig::Http(McpHttpServerConfig { url, .. })) if url == "https://example.com/mcp"
        ));
    }

    #[test]
    fn transport_name_matches_variant() {
        assert_eq!(
            McpServerConfig::Stdio(McpStdioServerConfig {
                command: "python".into(),
                args: vec![],
                env: None,
                cwd: None,
            })
            .transport(),
            "stdio"
        );
        assert_eq!(
            McpServerConfig::Http(McpHttpServerConfig {
                url: "https://example.com".into(),
                headers: HashMap::new(),
            })
            .transport(),
            "http"
        );
        assert_eq!(
            McpServerConfig::Ws(McpWebSocketServerConfig {
                url: "wss://example.com".into(),
                headers: HashMap::new(),
            })
            .transport(),
            "ws"
        );
    }
}
