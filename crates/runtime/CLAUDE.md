# lattice-runtime

## Purpose

Implements the agent control loop — the central orchestrator that loads event history, calls the LLM for decisions, routes tool calls, and records results. It is stateless and recovers all state from the SessionStore.

## Key Types

- `ControlLoop` — the agent brain, drives the decision cycle. Receives `Arc<dyn SessionStore>`, `Arc<dyn LLMClient>`, and `Arc<ToolSet>`. Run via `.run(session_id).await`.

## Design Decisions

- **Stateless**: ControlLoop holds no persistent state. All state is reconstructed from `SessionStore` on each iteration.
- **Event-sourced**: Every LLM call, tool call, and result is appended as an immutable event to the session log.
- **ToolSet abstraction**: All tools (bash, file, glob, etc.) are behind a unified `ToolSet` interface. ControlLoop never knows whether a tool runs in-process or in a subprocess sandbox.
- **No model-specific hardcoding**: Framework code must never contain logic branching on the LLM model name or version.

## Patterns

- `ControlLoop::run()` fetches all events for a session, converts them to LLM messages, calls the LLM, parses the response into decisions, executes tool calls, and appends results back to the session store.
- Tool calls are correlated to events via `parent_event_id`.
- All error paths emit `ToolCallError` or `RunError` events rather than returning errors up the stack.

## Known Issues

- [#26](https://github.com/hhllhhyyds/Lattice/issues/26) — ControlLoop reloads ALL events every iteration → O(n²) performance
- [#31](https://github.com/hhllhhyyds/Lattice/issues/31) — Thinking events leak into LLM conversation as assistant messages
- [#32](https://github.com/hhllhhyyds/Lattice/issues/32) — ToolError type information lost when recording ToolCallError events

## Dependencies

- Depends on: `lattice-core`, `lattice-llm-protocol`, `lattice-tools`
- Depended on by: `lattice` (facade), `lattice-server`