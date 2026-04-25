//! Basic sandbox router.

use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{
    Actor, EventId, EventPayload, ExecutionResult, RouterError, Sandbox, SandboxRouter, SessionId,
    SessionStore,
};
use tracing::instrument;

/// Default router that forwards all tool calls to a single sandbox.
///
/// Records execution results back into the SessionStore.
pub struct BasicSandboxRouter {
    sandbox: Arc<dyn Sandbox>,
    store: Arc<dyn SessionStore>,
}

impl BasicSandboxRouter {
    /// Create a new BasicSandboxRouter.
    #[must_use]
    pub fn new(sandbox: Arc<dyn Sandbox>, store: Arc<dyn SessionStore>) -> Self {
        Self { sandbox, store }
    }
}

#[async_trait]
impl SandboxRouter for BasicSandboxRouter {
    /// Execute a tool via the sandbox and record the result as an event.
    #[instrument(skip(self))]
    async fn route(
        &self,
        session_id: SessionId,
        parent_event_id: EventId,
        tool: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, RouterError> {
        let result = self
            .sandbox
            .execute(tool, params)
            .await
            .map_err(|e| RouterError::ExecutionFailed(e.to_string()))?;

        // Record the result as an event in the store.
        let payload = EventPayload::ToolCallResult {
            stdout: result.stdout.clone(),
            stderr: result.stderr.clone(),
            exit_code: result.exit_code,
        };
        let _ = self
            .store
            .append_event(session_id, payload, Actor::Sandbox, Some(parent_event_id))
            .await;

        Ok(result)
    }
}
