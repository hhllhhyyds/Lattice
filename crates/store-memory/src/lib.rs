//! Lattice in-memory store: development and testing implementation.
//!
//! Provides [`MemoryStore`], a session store backed by an in-memory `HashMap`.
//! Suitable for development, testing, and single-process workloads.
//!
//! # Not for production
//!
//! Data is not persisted. Process restarts will lose all session data.
//! Use a persistent store (SQLite, Postgres, etc.) for production.

mod memory_store;

pub use memory_store::MemoryStore;
