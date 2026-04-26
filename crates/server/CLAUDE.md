# lattice-server

## Purpose

HTTP API server built on axum. Exposes the Lattice agent framework as a REST API with session management, event querying, and (in the future) message submission and run triggering. Supports multiple LLM providers via feature flags.

## Key Types

- `AppState` — global shared state: `store`, `active_runs`, `started_at`, `sessions`
- `Router` — built via `router()` / `app()`, mounts `/health` and `/v1` route groups
- `RunHandle` / `RunStatus` — tracks in-flight agent runs with abort support

## API Routes

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check (status, version, uptime, enabled features) |
| POST | `/v1/sessions` | Create a new session |
| GET | `/v1/sessions` | List all sessions |
| GET | `/v1/sessions/:id` | Get session details |
| GET | `/v1/sessions/:id/events` | Get events (supports `actor`, `eventType`, `after`, `limit` filters) |
| POST | `/v1/sessions/:id/runs` | *(planned)* Trigger an agent run |

## Design Decisions

- State is shared via `Arc<AppState>`. `active_runs` and `sessions` use `Arc<RwLock<...>>` for interior mutability.
- Routes are registered under `/v1` prefix via `v1_routes()`.
- CORS is open (`allow_origin(Any)`) for development convenience.
- Feature-gated LLM providers: `anthropic` and `openai` features control which clients are compiled.

## Patterns

- All handler functions return `Result<Json<T>, AppError>`.
- Error responses follow `{ error: { code: "...", message: "..." } }` format.
- Feature flags: `default = anthropic + openai`; individual providers can be disabled.

## Known Issues

- [#29](https://github.com/hhllhhyyds/Lattice/issues/29) — Server API is read-only, missing message submission and run trigger endpoints
- [#37](https://github.com/hhllhhyyds/Lattice/issues/37) — Server startup banner shows "UNKEN" instead of "LATTICE"
- [#38](https://github.com/hhllhhyyds/Lattice/issues/38) — Server lacks graceful shutdown handling

## Dependencies

- Depends on: `lattice-core`, `lattice-runtime`, `lattice-store-memory`
- Depended on by: (public-facing service)