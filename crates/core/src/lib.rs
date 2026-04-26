//! Lattice core: pure trait definitions and shared types.
//!
//! This crate contains zero runtime dependencies — only interfaces.
//! All implementations live in separate crates.

pub mod error;
pub mod event;
pub mod llm;
pub mod sandbox;
pub mod session;
pub mod tool;

pub mod filter {
    //! Event filtering types.

    use crate::event::Actor;

    /// Filter for querying events from a SessionStore.
    #[derive(Debug, Clone, Default)]
    pub struct EventFilter {
        /// Filter by actor type.
        pub actor: Option<Actor>,
        /// Filter by event payload variant name (e.g. "ToolCallRequested").
        pub payload_type: Option<&'static str>,
    }
}

// Re-exports for convenience.
pub use error::{LLMError, SandboxError, StoreError, ToolError};
pub use event::{Actor, Event, EventId, EventPayload, SessionId, Timestamp};
pub use filter::EventFilter;
pub use llm::{Decision, LLMClient};
pub use sandbox::{ExecutionResult, Sandbox};
pub use session::SessionStore;
pub use tool::{ToolDescription, ToolExecutor};
