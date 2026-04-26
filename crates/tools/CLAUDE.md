# lattice-tools

## Purpose

Standard tool library for Lattice. Provides the `ToolSet` registry and built-in tool implementations (bash, file, glob, grep, http). All tools implement `lattice_core::ToolExecutor`.

## Key Types

- `ToolSet` — registry of available tools, indexed by name. Used by `ControlLoop` to route tool calls.
- `BashTool` — executes shell commands in a subprocess sandbox. Currently holds `Arc<dyn Sandbox>` but this creates a dependency coupling issue (see #36).

## Tool Layer Architecture

```
Layer 1 (core):       ToolExecutor trait (interface only)
Layer 2 (lattice-tools): Standard tool library (BashTool, FileTool, etc.)
Layer 3 (application): User-defined tools injected into ToolSet
```

`ControlLoop` calls `ToolSet::execute()` — it never distinguishes between in-process and sandboxed execution.

## Design Decisions

- Tools are registered at startup via `ToolSet::with_defaults()` or manually via `ToolSet::register()`.
- Each tool describes itself with a `ToolDescription` (name, description, input schema) returned by `ToolExecutor::describe()`.
- Tool execution errors are wrapped as `ToolError` and emitted as `ToolCallError` events.

## Known Issues

- [#36](https://github.com/hhllhhyyds/Lattice/issues/36) — BashTool holds Arc<dyn Sandbox> but lattice-tools depends on lattice-sandbox-local, creating circular coupling risk

## Dependencies

- Depends on: `lattice-core`
- Depended on by: `lattice-runtime`