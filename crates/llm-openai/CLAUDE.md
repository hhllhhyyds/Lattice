# lattice-llm-openai

## Purpose

Implements `lattice_core::LLMClient` using the OpenAI Chat Completions API format. Compatible with OpenAI, local deployments (vLLM, Ollama), and third-party proxies that follow the OpenAI tool-calling schema.

## Key Types

- `OpenAIClient` — the main client. Created via `OpenAIClient::new()` with a base URL and API key.

## Design Decisions

- Uses the OpenAI Chat Completions API (`/v1/chat/completions`).
- Tool calls use the OpenAI `function` calling schema. Multi-tool-call handling has a known bug — see #27.
- Base URL is configurable to support local deployments and proxies.
- Request timeout is hardcoded to 120s — see #34.

## Patterns

- Request building: converts `LLMRequest` from `lattice-llm-protocol` into OpenAI JSON payload.
- Response parsing: handles both text responses and tool call responses. Tool call results are correlated via `parent_event_id` — see #28.
- Streaming: not currently supported.

## Known Issues

- [#27](https://github.com/hhllhhyyds/Lattice/issues/27) — OpenAI client silently drops parallel tool calls
- [#28](https://github.com/hhllhhyyds/Lattice/issues/28) — Tool call ID correlation is fragile, should use `parent_event_id`
- [#34](https://github.com/hhllhhyyds/Lattice/issues/34) — HTTP timeout hardcoded to 120s

## Dependencies

- Depends on: `lattice-core`, `lattice-llm-protocol`
- Depended on by: `lattice-server`