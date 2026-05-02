//! MCP client support for Lattice.
//!
//! This crate provides the first-stage MCP integration layer:
//! configuration types, connection status models, and a stdio-backed
//! multi-server client manager.

mod manager;
mod resource_tools;
mod tool_adapter;
mod types;

pub use manager::{McpClientManager, McpToolCallOutput};
pub use resource_tools::{ListMcpResourcesTool, ReadMcpResourceTool};
pub use tool_adapter::{mcp_tool_name, McpToolAdapter};
pub use types::{
    McpConnectionSnapshot, McpConnectionState, McpConnectionStatus, McpHttpServerConfig,
    McpJsonConfig, McpResourceInfo, McpServerConfig, McpStdioServerConfig, McpToolInfo,
    McpWebSocketServerConfig,
};
