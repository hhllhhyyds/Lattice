//! MCP client support for Lattice.
//!
//! This crate provides the first-stage MCP integration layer:
//! configuration types, connection status models, and a stdio-backed
//! multi-server client manager.

mod integration;
mod manager;
mod resource_tools;
mod tool_adapter;
mod types;

pub use integration::{
    load_mcp_manager_from_env, load_mcp_server_configs_from_env, load_mcp_server_configs_from_path,
    register_mcp_tools, LATTICE_MCP_CONFIG_ENV,
};
pub use manager::{McpClientManager, McpToolCallOutput};
pub use resource_tools::{ListMcpResourcesTool, ReadMcpResourceTool};
pub use tool_adapter::{mcp_tool_name, McpToolAdapter};
pub use types::{
    McpConnectionSnapshot, McpConnectionState, McpConnectionStatus, McpHttpServerConfig,
    McpJsonConfig, McpResourceInfo, McpServerConfig, McpStdioServerConfig, McpToolInfo,
    McpWebSocketServerConfig,
};
