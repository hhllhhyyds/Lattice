# lattice-llm-protocol

## Purpose

Provider-agnostic message types and conversion logic. Bridges Lattice's event-sourced architecture and the various LLM provider APIs. This is the single translation layer — LLM clients delegate to it rather than implementing conversion themselves.

## Key Types

- `Message` / `ContentBlock` / `Role` — universal message format used by all LLM clients
- `events_to_messages()` — converts Lattice `Event` stream into LLM message list
- `response_to_decision()` — parses LLM raw response into `lattice_core::Decision`
- `LLMRequest` / `LLMResponse` — provider-agnostic request/response wrappers
- `ToolSpec` — describes a tool for the LLM (name, description, input schema)

## Design Decisions

- All message conversion lives here — `AnthropicClient` and `OpenAIClient` call into this crate rather than duplicating conversion logic.
- `Role::System` is treated specially: in most providers it maps to a system message, but AnthropicClient has a known bug where it silently maps system to user role (see #33).
- Events are filtered before conversion: internal events (run lifecycle, error states) do not become LLM messages. Only user input and model output become conversation context.

## Patterns

- `convert::` module: event → message conversion
- `parse::` module: response → decision parsing
- `request::` / `response::` modules: wire format types

## Known Issues

- [#28](https://github.com/hhllhhyyds/Lattice/issues/28) — Tool call ID correlation is fragile, should use `parent_event_id`
- [#31](https://github.com/hhllhhyyds/Lattice/issues/31) — Thinking events leak into LLM conversation as assistant messages

## Dependencies

- Depends on: `lattice-core`
- Depended on by: `lattice-runtime`, `lattice-llm-anthropic`, `lattice-llm-openai`