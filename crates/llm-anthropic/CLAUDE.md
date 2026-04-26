# lattice-llm-anthropic

## Purpose

Implements `lattice_core::LLMClient` using the Anthropic Messages API. Wraps HTTP calls to the Anthropic API, serializes requests using types from `lattice-llm-protocol`, and parses responses back into Lattice decisions.

## Key Types

- `AnthropicClient` — the main client. Created via `AnthropicClient::new()`. Takes an HTTP client (default: reqwest) and API credentials.

## Design Decisions

- Uses the Anthropic Messages API (not the older Completions API).
- API key is read from the `ANTHROPIC_API_KEY` environment variable.
- Request timeout is hardcoded to 120s — see #34.
- System messages: Anthropic uses a dedicated `system` role in the messages array. There is a known bug where system messages are silently remapped to the user role — see #33.

## Patterns

- Request building: converts `LLMRequest` from `lattice-llm-protocol` into Anthropic-specific JSON payload.
- Response parsing: deserializes Anthropic response body, then calls `response_to_decision()` from `lattice-llm-protocol`.
- Error handling: HTTP errors are wrapped as `LlmError::RequestFailed` variants.

## Known Issues

- [#33](https://github.com/hhllhhyyds/Lattice/issues/33) — AnthropicClient silently maps Role::System to user role
- [#34](https://github.com/hhllhhyyds/Lattice/issues/34) — HTTP timeout hardcoded to 120s

## Dependencies

- Depends on: `lattice-core`, `lattice-llm-protocol`
- Depended on by: `lattice-server`