//! Core traits for Lattice's three-component architecture.

use async_trait::async_trait;

use crate::{
    error::{LLMError, RouterError, SandboxError, StoreError},
    Actor, Decision, Event, EventFilter, EventId, EventPayload, ExecutionResult, SessionId,
    ToolDescription,
};

/// Session store — event log persistence.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Create a new session and return its id.
    async fn create_session(&self) -> Result<SessionId, StoreError>;

    /// Append an immutable event to the session.
    async fn append_event(
        &self,
        session_id: SessionId,
        payload: EventPayload,
        actor: Actor,
        parent_event_id: Option<EventId>,
    ) -> Result<EventId, StoreError>;

    /// Retrieve events for a session.
    async fn get_events(
        &self,
        session_id: SessionId,
        filter: &EventFilter,
    ) -> Result<Vec<Event>, StoreError>;

    /// Get the latest event id for a session.
    async fn latest_event_id(&self, session_id: SessionId) -> Result<Option<EventId>, StoreError>;
}

/// LLM client — decision making.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Decide the next action based on event history.
    async fn decide(
        &self,
        history: &[Event],
        available_tools: &[ToolDescription],
        system_prompt: &str,
    ) -> Result<Decision, LLMError>;
}

/// Sandbox — isolated tool execution environment.
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Execute a command in the sandbox.
    async fn execute(
        &self,
        command: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, SandboxError>;
}

/// Sandbox router — routes tool calls to the appropriate sandbox.
#[async_trait]
pub trait SandboxRouter: Send + Sync {
    /// Route a tool call to a sandbox and record the result as an event.
    async fn route(
        &self,
        session_id: SessionId,
        parent_event_id: EventId,
        tool: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, RouterError>;
}
