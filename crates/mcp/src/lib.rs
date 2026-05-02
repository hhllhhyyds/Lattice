//! MCP client support for Lattice.
//!
//! This crate provides the first-stage MCP integration layer:
//! configuration types, connection status models, and a stdio-backed
//! multi-server client manager.

mod manager;
mod types;

pub use manager::McpClientManager;
pub use types::{
    McpConnectionState, McpConnectionStatus, McpHttpServerConfig, McpJsonConfig, McpResourceInfo,
    McpServerConfig, McpStdioServerConfig, McpToolInfo, McpWebSocketServerConfig,
};
