//! Session API handlers and routing.

pub mod sessions;
pub mod types;

/// Re-exports the v1 route aggregator.
pub use sessions::v1_routes;
