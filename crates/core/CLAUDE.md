# lattice-core

## Purpose

Pure interface layer for Lattice. Defines all core traits and types with zero external dependencies beyond standard Rust crates (serde, uuid, chrono, async-trait). No feature flags — always compiled in full.

## Key Types

### Traits
- `SessionStore` — event log persistence. Methods: `create_session`, `delete_session`, `append_event`, `get_events`, `latest_event_id`
- `LLMClient` — decision making. Single method: `decide(history, available_tools, system_prompt) -> Decision`
- `Sandbox` — isolated tool execution. Single method: `execute(command, params) -> ExecutionResult`
- `ToolExecutor` — individual tool implementation. Methods: `description() -> ToolDescription`, `execute(params) -> ExecutionResult`

### Enums
- `EventPayload` — all event variants: `SessionCreated`, `UserMessage`, `Thinking`, `ToolCallRequested`, `ToolCallResult`, `ToolCallError`, `FinalAnswer`, `StateChange`
- `Decision` — LLM decision variants: `Thinking`, `ToolCall`, `ThinkingToolCall`, `MultiToolCall`, `FinalAnswer`
- `Actor` — event producer: `System`, `LLM`, `Harness`, `Sandbox`
- `ToolErrorKind` — structured tool failure categories: `NotFound`, `InvalidParams`, `ExecutionFailed`, `Timeout`, `Other`

### Structs
- `Event` — immutable append-only record: `event_id`, `session_id`, `timestamp`, `actor`, `payload`, `parent_event_id`
- `ToolDescription` — LLM-facing tool spec: `name`, `description`, `parameters_schema`
- `ToolCallRequest` — single call within a `MultiToolCall`: `id`, `tool`, `params`
- `ExecutionResult` — tool output: `stdout`, `stderr`, `exit_code`
- `EventFilter` — query filter for `get_events` (time range, payload type filters)

### Error Types
- `StoreError` — SessionStore failures
- `LLMError` — LLMClient failures
- `SandboxError` — Sandbox failures
- `ToolError` — ToolExecutor failures

## Design Decisions

- Zero feature flags: core is always fully compiled; optional functionality lives in implementation crates.
- `ToolErrorKind` is embedded in `EventPayload::ToolCallError` to preserve structured failure information in the event log.
- `Thinking` in `EventPayload` carries an optional `signature` field for LLM providers (e.g. Anthropic extended thinking) that require round-trip opaque tokens.

## Dependencies

- Depends on: `serde`, `uuid`, `chrono`, `async-trait`
- Depended on by: all other Lattice crates
