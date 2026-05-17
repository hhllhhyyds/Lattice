# lattice-server

## Purpose

HTTP API server built on axum. Exposes the Lattice agent framework as a REST API with session management, event querying, message submission, and agent run triggering. Supports multiple LLM providers via feature flags.

## Key Types

- `AppState` — global shared state: `store`, `active_runs`, `started_at`, `sessions`
- `Router` — built via `router()` / `app()`, mounts `/health` and `/v1` route groups
- `RunHandle` / `RunStatus` — tracks in-flight agent runs with abort support
- `SubmitMessageRequest` / `SubmitMessageResponse` — message submission and run triggering
- `MessagesResponse` / `StatusResponse` — conversation history and execution status

## API Routes

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check (status, version, uptime, enabled features) |
| POST | `/v1/sessions` | Create a new session |
| GET | `/v1/sessions` | List all sessions |
| GET | `/v1/sessions/:id` | Get session details |
| GET | `/v1/sessions/:id/events` | Get events (supports `actor`, `eventType`, `after`, `limit` filters) |
| POST | `/v1/sessions/:id/messages` | Submit user message and trigger agent execution (returns 202 Accepted) |
| GET | `/v1/sessions/:id/messages` | Get conversation history (UserMessage + FinalAnswer events) |
| GET | `/v1/sessions/:id/status` | Query execution status (idle/running/completed/failed) |

## Design Decisions

- State is shared via `Arc<AppState>`. `active_runs` and `sessions` use `Arc<RwLock<...>>` for interior mutability.
- Routes are registered under `/v1` prefix via `v1_routes()`.
- CORS is open (`allow_origin(Any)`) for development convenience.
- Feature-gated LLM providers: `anthropic` and `openai` features control which clients are compiled.

## Patterns

- All handler functions return `Result<Json<T>, AppError>` or `Result<(StatusCode, Json<T>), AppError>`.
- Error responses follow `{ error: { code: "...", message: "..." } }` format.
- Feature flags: `default = anthropic + openai`; individual providers can be disabled.
- Agent runs are spawned as tokio tasks and tracked via `RunHandle` in `active_runs`.
- Concurrent execution per session is prevented (returns 409 Conflict).

## Known Issues

- [#37](https://github.com/hhllhhyyds/Lattice/issues/37) — Server startup banner shows "UNKEN" instead of "LATTICE"
- [#38](https://github.com/hhllhhyyds/Lattice/issues/38) — Server lacks graceful shutdown handling
- Agent execution currently uses mock task (50ms sleep) instead of real ControlLoop
- LLM provider/model/system_prompt parameters are accepted but not yet used

## Dependencies

- Depends on: `lattice-core`, `lattice-runtime`, `lattice-store-memory`, `lattice-sandbox-local`, `lattice-tools`
- Depended on by: (public-facing service)

## Recent Changes

### Task 13.1: Web UI Markdown Rendering (2026-05-17)
- `index.html`: load `marked` and `DOMPurify` via jsDelivr CDN
- `app.js`: `renderMessages` now renders assistant messages as markdown (`marked.parse` → `DOMPurify.sanitize` → `div.markdown-body`); user messages unchanged (`<pre>` + `escapeHtml`)
- `app.css`: added `.markdown-body` prose typography block (headings, lists, code, blockquotes, tables)
- 6 new TDD tests added, all 52 lib tests passing

### Task 16: Agent Run API (2026-04-28)
- Added POST `/v1/sessions/:id/messages` for message submission and agent triggering
- Added GET `/v1/sessions/:id/messages` for conversation history
- Added GET `/v1/sessions/:id/status` for execution status querying
- Added `Conflict` error type (409) for concurrent run detection
- Implemented `RunHandle` registration and tracking
- 13 integration tests added, all passing