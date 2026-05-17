# lattice-mcp

## Purpose

MCP (Model Context Protocol) client integration for Lattice. Provides a multi-server stdio client manager and a `ToolExecutor` adapter that bridges MCP tools into Lattice's tool system.

## Key Types

- `McpClientManager` — manages connections to multiple MCP servers. Spawns stdio subprocesses, tracks connection state, and multiplexes tool calls.
- `McpToolCallOutput` — result of an MCP tool invocation.
- `McpToolAdapter` — implements `ToolExecutor`, wrapping a single MCP tool so it can be registered in a `ToolSet`.
- `ListMcpResourcesTool` / `ReadMcpResourceTool` — `ToolExecutor` implementations for listing and reading MCP resources.

### Config Types
- `McpJsonConfig` — top-level config (maps server name → `McpServerConfig`)
- `McpServerConfig` — enum: `Stdio(McpStdioServerConfig)`, `Http(McpHttpServerConfig)`, `WebSocket(McpWebSocketServerConfig)`
- `McpStdioServerConfig` — command + args + optional env for stdio servers
- `McpConnectionStatus` / `McpConnectionSnapshot` — observable connection state

### Integration Helpers
- `load_mcp_server_configs_from_env` / `load_mcp_server_configs_from_path` — load config from `LATTICE_MCP_CONFIG` env var or a JSON file path
- `load_mcp_manager_from_env` — convenience: load config + construct `McpClientManager`
- `register_mcp_tools` — register all connected MCP server tools into a `ToolSet`
- `mcp_tool_name` — canonical tool name used when registering MCP tools (`<server>__<tool>`)
- `LATTICE_MCP_CONFIG_ENV` — env var name constant (`"LATTICE_MCP_CONFIG"`)

## Design Decisions

- MCP tools are a Layer 3 injection: the application calls `register_mcp_tools` to populate a `ToolSet`, then passes the set to `ControlLoop`. The control loop treats MCP tools identically to built-in tools.
- Tool names are namespaced as `<server_name>__<tool_name>` to avoid collisions when multiple MCP servers expose tools with the same name.

## Dependencies

- Depends on: `tokio`, `lattice-core` (via `ToolExecutor`)
- Depended on by: `lattice-server` (optional), application layer
