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

    use crate::event::{Actor, EventId, Timestamp};

    /// Filter for querying events from a SessionStore.
    #[derive(Debug, Clone, Default)]
    pub struct EventFilter {
        /// Filter by actor type.
        pub actor: Option<Actor>,
        /// Filter by event payload variant name (e.g. "toolCallRequested").
        pub payload_type: Option<&'static str>,
        /// Return events after this event id.
        pub after_event_id: Option<EventId>,
        /// Return events at or after this timestamp.
        pub since: Option<Timestamp>,
        /// Return events at or before this timestamp.
        pub until: Option<Timestamp>,
        /// Maximum number of events to return.
        pub limit: Option<usize>,
    }
}

// Re-exports for convenience.
pub use error::{LLMError, SandboxError, StoreError, ToolError};
pub use event::{Actor, Event, EventId, EventPayload, SessionId, Timestamp, ToolErrorKind};
pub use filter::EventFilter;
pub use llm::{Decision, LLMClient};
pub use sandbox::{ExecutionResult, Sandbox};
pub use session::SessionStore;
pub use tool::{ToolDescription, ToolExecutor};
