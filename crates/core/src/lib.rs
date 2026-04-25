//! Lattice core: pure trait definitions and shared types.
//!
//! This crate contains zero runtime dependencies — only interfaces.
//! All implementations live in separate crates.

pub mod error;
pub mod event;
pub mod llm;
pub mod router;
pub mod sandbox;
pub mod session;

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

pub mod tool {
    //! Tool-related types.

    use serde::{Deserialize, Serialize};

    /// Tool description injected to the LLM.
    ///
    /// Describes a callable tool so the LLM can decide when and how to use it.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolDescription {
        /// Tool name (must be unique within a session).
        pub name: String,
        /// Human-readable description of what the tool does.
        pub description: String,
        /// JSON Schema describing the tool's parameters.
        pub parameters_schema: serde_json::Value,
    }
}

// Re-exports for convenience.
pub use error::{LLMError, RouterError, SandboxError, StoreError};
pub use event::{Actor, Event, EventId, EventPayload, SessionId, Timestamp};
pub use filter::EventFilter;
pub use llm::{Decision, LLMClient};
pub use router::SandboxRouter;
pub use sandbox::{ExecutionResult, Sandbox};
pub use session::SessionStore;
pub use tool::ToolDescription;
