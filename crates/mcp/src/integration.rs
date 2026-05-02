use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use lattice_core::ToolError;
use lattice_tools::ToolSet;

use crate::{
    ListMcpResourcesTool, McpClientManager, McpJsonConfig, McpServerConfig, McpToolAdapter,
    ReadMcpResourceTool,
};

pub const LATTICE_MCP_CONFIG_ENV: &str = "LATTICE_MCP_CONFIG";

pub fn load_mcp_server_configs_from_path(
    path: impl AsRef<Path>,
) -> Result<HashMap<String, McpServerConfig>, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|err| format!("failed to read MCP config '{}': {err}", path.display()))?;
    let parsed: McpJsonConfig = serde_json::from_str(&content)
        .map_err(|err| format!("failed to parse MCP config '{}': {err}", path.display()))?;
    Ok(parsed.mcp_servers)
}

pub fn load_mcp_server_configs_from_env() -> Result<HashMap<String, McpServerConfig>, String> {
    let Some(path) = env::var_os(LATTICE_MCP_CONFIG_ENV) else {
        return Ok(HashMap::new());
    };
    load_mcp_server_configs_from_path(path)
}

pub async fn load_mcp_manager_from_env() -> Result<Option<Arc<McpClientManager>>, String> {
    let configs = load_mcp_server_configs_from_env()?;
    if configs.is_empty() {
        return Ok(None);
    }

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;
    Ok(Some(Arc::new(manager)))
}

pub fn register_mcp_tools(
    toolset: &mut ToolSet,
    manager: Arc<McpClientManager>,
) -> Result<(), ToolError> {
    toolset.register(ListMcpResourcesTool::new(manager.clone()))?;
    toolset.register(ReadMcpResourceTool::new(manager.clone()))?;
    for tool in McpToolAdapter::from_manager(manager) {
        toolset.register(tool)?;
    }
    Ok(())
}
