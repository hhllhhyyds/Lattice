//! Sandbox router types and the SandboxRouter trait.

use async_trait::async_trait;

use crate::error::RouterError;
use crate::sandbox::ExecutionResult;
use crate::{EventId, SessionId};

/// Sandbox router — routes tool calls to the appropriate sandbox.
///
/// The router is responsible for selecting which sandbox instance
/// should handle a given tool call, and recording execution results
/// back into the session store.
#[async_trait]
pub trait SandboxRouter: Send + Sync {
    /// Route a tool call to a sandbox and return the execution result.
    ///
    /// Implementations should record the result as an event in the SessionStore.
    async fn route(
        &self,
        session_id: SessionId,
        parent_event_id: EventId,
        tool: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, RouterError>;
}
