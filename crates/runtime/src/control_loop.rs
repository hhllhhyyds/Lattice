//! The agent control loop.

use std::sync::Arc;

use lattice_core::{
    Actor, Decision, EventFilter, EventPayload, LLMClient, SandboxRouter, SessionId, SessionStore,
};
use tracing::{info, instrument};

/// Control loop — the agent's brain.
///
/// Loads event history, calls the LLM for decisions, routes tool calls,
/// and records results. All state is recovered from the SessionStore.
pub struct ControlLoop {
    store: Arc<dyn SessionStore>,
    llm: Arc<dyn LLMClient>,
    router: Arc<dyn SandboxRouter>,
}

impl ControlLoop {
    /// Create a new control loop.
    #[must_use]
    pub fn new(
        store: Arc<dyn SessionStore>,
        llm: Arc<dyn LLMClient>,
        router: Arc<dyn SandboxRouter>,
    ) -> Self {
        Self { store, llm, router }
    }

    /// Get a reference to the session store.
    pub fn store(&self) -> &Arc<dyn SessionStore> {
        &self.store
    }

    /// Run the control loop for a session.
    #[instrument(skip(self))]
    pub async fn run(&self, session_id: SessionId) -> anyhow::Result<()> {
        info!(?session_id, "control loop started");

        loop {
            let events = self
                .store
                .get_events(session_id, &EventFilter::default())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let decision = self
                .llm
                .decide(&events, &[], "You are a helpful agent.")
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            match decision {
                Decision::Thinking { reasoning } => {
                    info!(?reasoning, "LLM thinking");
                    let event_id = self
                        .store
                        .append_event(
                            session_id,
                            EventPayload::Thinking { reasoning },
                            Actor::LLM,
                            events.last().map(|e| e.event_id),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    let _ = event_id;
                }
                Decision::ToolCall { tool, params } => {
                    info!(?tool, "LLM requested tool call");
                    let parent_id = events.last().map(|e| e.event_id);
                    let req_event_id = self
                        .store
                        .append_event(
                            session_id,
                            EventPayload::ToolCallRequested {
                                tool: tool.clone(),
                                params: params.clone(),
                            },
                            Actor::LLM,
                            parent_id,
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;

                    let result = self
                        .router
                        .route(session_id, req_event_id, &tool, params)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;

                    self.store
                        .append_event(
                            session_id,
                            EventPayload::ToolCallResult {
                                stdout: result.stdout,
                                stderr: result.stderr,
                                exit_code: result.exit_code,
                            },
                            Actor::Sandbox,
                            Some(req_event_id),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                }
                Decision::FinalAnswer { answer } => {
                    info!(?answer, "LLM final answer");
                    self.store
                        .append_event(
                            session_id,
                            EventPayload::FinalAnswer { answer },
                            Actor::LLM,
                            events.last().map(|e| e.event_id),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    break;
                }
            }
        }

        info!(?session_id, "control loop finished");
        Ok(())
    }
}
