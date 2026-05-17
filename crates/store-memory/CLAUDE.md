# lattice-store-memory

## Purpose

In-memory implementation of `SessionStore`. Intended for development and testing only — all data is lost on process restart.

## Key Types

- `MemoryStore` — implements `SessionStore` using `Arc<RwLock<HashMap<SessionId, Vec<Event>>>>` for concurrent access.

## Behavior

- `create_session`: allocates a new UUID session and appends an initial `SessionCreated` event.
- `delete_session`: removes the session and all its events from the map.
- `append_event`: acquires a write lock and pushes an `Event` with a new UUID and current timestamp.
- `get_events`: acquires a read lock and applies `EventFilter` in memory.
- `latest_event_id`: returns the `event_id` of the last event in the session's vec, or `None` if empty.

## Design Decisions

- `Arc<RwLock<...>>` allows `MemoryStore` to be cloned cheaply — all clones share the same underlying map. This is intentional: the server crate creates one store and shares it across request handlers via `Arc`.
- Not suitable for production. Use a persistent backend (e.g. SQLite, Postgres) for anything that must survive restarts.

## Dependencies

- Depends on: `lattice-core`, `tokio` (RwLock), `chrono`, `uuid`, `async-trait`
- Depended on by: `lattice-runtime` (tests), `lattice-server`
