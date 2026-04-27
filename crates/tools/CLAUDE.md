# lattice-tools

## Purpose

Standard tool library for Lattice. Provides the `ToolSet` registry and built-in tool implementations (bash, file, glob, grep, http). All tools implement `lattice_core::ToolExecutor`.

## Key Types

- `ToolSet` — registry of available tools, indexed by name. Used by `ControlLoop` to route tool calls.
- `BashTool` — executes shell commands in a subprocess sandbox. Platform-aware: provides different tool descriptions based on the target OS.

## Tool Layer Architecture

```
Layer 1 (core):       ToolExecutor trait (interface only)
Layer 2 (lattice-tools): Standard tool library (BashTool, FileTool, etc.)
Layer 3 (application): User-defined tools injected into ToolSet
```

`ControlLoop` calls `ToolSet::execute()` — it never distinguishes between in-process and sandboxed execution.

## Platform-Aware Tools

BashTool is platform-aware and provides different tool descriptions based on the target platform:

- **Unix/Linux/macOS**: Tool name is `"sh"`, description includes Unix commands (ls, cat, grep, find)
- **Windows**: Tool name is `"cmd"`, description includes Windows commands (dir, type, findstr, where)

This ensures the LLM generates platform-appropriate commands. The platform detection happens at compile time using `#[cfg(unix)]` and `#[cfg(windows)]`.

### Example

On Unix:
```json
{
  "name": "sh",
  "description": "Execute a Unix shell command... Use commands like 'ls', 'cat', 'grep'..."
}
```

On Windows:
```json
{
  "name": "cmd",
  "description": "Execute a Windows cmd.exe command... Use commands like 'dir', 'type', 'findstr'..."
}
```

## Design Decisions

- Tools are registered at startup via `ToolSet::with_defaults()` or manually via `ToolSet::register()`.
- Each tool describes itself with a `ToolDescription` (name, description, input schema) returned by `ToolExecutor::describe()`.
- Tool execution errors are wrapped as `ToolError` and emitted as `ToolCallError` events.
- Platform detection is compile-time, not runtime, for zero overhead.

## Known Issues

- [#36](https://github.com/hhllhhyyds/Lattice/issues/36) — BashTool holds Arc<dyn Sandbox> but lattice-tools depends on lattice-sandbox-local, creating circular coupling risk

## Dependencies

- Depends on: `lattice-core`
- Depended on by: `lattice-runtime`
