//! Basic sandbox router.

use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{Sandbox, SandboxRouter, SessionStore};

/// Default router that forwards all tool calls to a single sandbox.
pub struct BasicSandboxRouter {
    sandbox: Arc<dyn Sandbox>,
    _store: Arc<dyn SessionStore>,
}

impl BasicSandboxRouter {
    #[must_use]
    pub fn new(sandbox: Arc<dyn Sandbox>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            sandbox,
            _store: store,
        }
    }
}

#[async_trait]
impl SandboxRouter for BasicSandboxRouter {
    async fn route(
        &self,
        _session_id: lattice_core::SessionId,
        _parent_event_id: lattice_core::EventId,
        tool: &str,
        params: serde_json::Value,
    ) -> Result<lattice_core::ExecutionResult, lattice_core::RouterError> {
        // Forward to the sandbox. The tool name becomes the command.
        self.sandbox
            .execute(tool, params)
            .await
            .map_err(|e| lattice_core::RouterError(e.to_string()))
    }
}
