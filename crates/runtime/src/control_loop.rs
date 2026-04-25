//! The agent control loop.

use std::sync::Arc;

use lattice_core::{
    Actor, Decision, EventFilter, EventPayload, LLMClient, SandboxRouter, SessionId, SessionStore,
    ToolDescription,
};
use tracing::{info, instrument, warn};

/// The maximum number of decision cycles before forcing exit.
const DEFAULT_MAX_ITERATIONS: usize = 50;

/// Control loop — the agent's brain.
///
/// Loads event history, calls the LLM for decisions, routes tool calls,
/// and records results. All state is recovered from the SessionStore.
///
/// The loop terminates on `FinalAnswer` or when [`max_iterations`] is reached.
pub struct ControlLoop {
    store: Arc<dyn SessionStore>,
    llm: Arc<dyn LLMClient>,
    router: Arc<dyn SandboxRouter>,
    available_tools: Vec<ToolDescription>,
    system_prompt: String,
    max_iterations: usize,
}

impl ControlLoop {
    /// Create a new control loop with the minimum required components.
    ///
    /// Uses defaults: empty tools, `"You are a helpful agent."` prompt,
    /// and 50 max iterations.
    #[must_use]
    pub fn new(
        store: Arc<dyn SessionStore>,
        llm: Arc<dyn LLMClient>,
        router: Arc<dyn SandboxRouter>,
    ) -> Self {
        Self {
            store,
            llm,
            router,
            available_tools: Vec::new(),
            system_prompt: "You are a helpful agent.".to_string(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }

    /// Create a new control loop with all options.
    #[must_use]
    pub fn with_options(
        store: Arc<dyn SessionStore>,
        llm: Arc<dyn LLMClient>,
        router: Arc<dyn SandboxRouter>,
        available_tools: Vec<ToolDescription>,
        system_prompt: String,
        max_iterations: usize,
    ) -> Self {
        Self {
            store,
            llm,
            router,
            available_tools,
            system_prompt,
            max_iterations,
        }
    }

    /// Get a reference to the session store.
    pub fn store(&self) -> &Arc<dyn SessionStore> {
        &self.store
    }

    /// Run the control loop for a session.
    ///
    /// Returns the final answer string on success.
    #[instrument(skip(self))]
    pub async fn run(&self, session_id: SessionId) -> anyhow::Result<String> {
        info!(?session_id, "control loop started");

        for _ in 0..self.max_iterations {
            let events = self
                .store
                .get_events(session_id, &EventFilter::default())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let decision = self
                .llm
                .decide(&events, &self.available_tools, &self.system_prompt)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            match decision {
                Decision::Thinking { reasoning } => {
                    info!(?reasoning, "LLM thinking");
                    self.store
                        .append_event(
                            session_id,
                            EventPayload::Thinking { reasoning },
                            Actor::LLM,
                            events.last().map(|e| e.event_id),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
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

                    // Router executes and records ToolCallResult on success.
                    // On failure, we record ToolCallError here.
                    if let Err(e) = self
                        .router
                        .route(session_id, req_event_id, &tool, params)
                        .await
                    {
                        self.store
                            .append_event(
                                session_id,
                                EventPayload::ToolCallError {
                                    error: e.to_string(),
                                },
                                Actor::Sandbox,
                                Some(req_event_id),
                            )
                            .await
                            .map_err(|e| anyhow::anyhow!("{e}"))?;
                    }
                }
                Decision::FinalAnswer { answer } => {
                    info!(?answer, "LLM final answer");
                    self.store
                        .append_event(
                            session_id,
                            EventPayload::FinalAnswer {
                                answer: answer.clone(),
                            },
                            Actor::LLM,
                            events.last().map(|e| e.event_id),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    info!(?session_id, "control loop finished");
                    return Ok(answer);
                }
            }
        }

        warn!(
            session_id = ?session_id,
            iterations = self.max_iterations,
            "max iterations reached"
        );
        Err(anyhow::anyhow!(
            "max iterations ({}) reached",
            self.max_iterations
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use lattice_core::{
        Actor, Decision, Event, EventFilter, EventPayload, ExecutionResult, LLMClient, LLMError,
        RouterError, SandboxRouter, SessionId, ToolDescription,
    };

    /// Manual test double for SessionStore.
    struct TestStore {
        sessions: Arc<Mutex<HashMap<SessionId, Vec<Event>>>>,
    }

    impl TestStore {
        fn new() -> Self {
            Self {
                sessions: Arc::new(Mutex::new(HashMap::new())),
            }
        }
        fn insert_session(&self, session_id: SessionId, events: Vec<Event>) {
            self.sessions.lock().unwrap().insert(session_id, events);
        }
    }

    #[async_trait]
    impl lattice_core::SessionStore for TestStore {
        async fn create_session(&self) -> Result<SessionId, lattice_core::error::StoreError> {
            let session_id = SessionId::new_v4();
            let event = Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::SessionCreated,
                parent_event_id: None,
            };
            self.sessions
                .lock()
                .unwrap()
                .insert(session_id, vec![event]);
            Ok(session_id)
        }
        async fn append_event(
            &self,
            session_id: SessionId,
            payload: EventPayload,
            actor: Actor,
            parent_event_id: Option<lattice_core::EventId>,
        ) -> Result<lattice_core::EventId, lattice_core::error::StoreError> {
            let event_id = lattice_core::EventId::new_v4();
            let event = Event {
                event_id,
                session_id,
                timestamp: chrono::Utc::now(),
                actor,
                payload,
                parent_event_id,
            };
            let mut sessions = self.sessions.lock().unwrap();
            let events = sessions
                .get_mut(&session_id)
                .ok_or(lattice_core::error::StoreError::SessionNotFound(session_id))?;
            events.push(event);
            Ok(event_id)
        }
        async fn get_events(
            &self,
            session_id: SessionId,
            filter: &EventFilter,
        ) -> Result<Vec<Event>, lattice_core::error::StoreError> {
            let sessions = self.sessions.lock().unwrap();
            let events = sessions
                .get(&session_id)
                .ok_or(lattice_core::error::StoreError::SessionNotFound(session_id))?;
            let mut result = events.clone();
            if let Some(actor) = filter.actor {
                result.retain(|e| e.actor == actor);
            }
            Ok(result)
        }
        async fn latest_event_id(
            &self,
            session_id: SessionId,
        ) -> Result<Option<lattice_core::EventId>, lattice_core::error::StoreError> {
            let sessions = self.sessions.lock().unwrap();
            let events = sessions
                .get(&session_id)
                .ok_or(lattice_core::error::StoreError::SessionNotFound(session_id))?;
            Ok(events.last().map(|e| e.event_id))
        }
    }

    /// Manual test double for LLMClient.
    struct TestLLM {
        decision: Decision,
    }

    impl TestLLM {
        fn new(decision: Decision) -> Self {
            Self { decision }
        }
    }

    #[async_trait]
    impl LLMClient for TestLLM {
        async fn decide(
            &self,
            _history: &[Event],
            _available_tools: &[ToolDescription],
            _system_prompt: &str,
        ) -> Result<Decision, LLMError> {
            Ok(self.decision.clone())
        }
    }

    /// Manual test double for SandboxRouter.
    struct TestRouter {
        result: Result<ExecutionResult, RouterError>,
    }

    impl TestRouter {
        fn new(result: Result<ExecutionResult, RouterError>) -> Self {
            Self { result }
        }
    }

    #[async_trait]
    impl SandboxRouter for TestRouter {
        async fn route(
            &self,
            _session_id: SessionId,
            _parent_event_id: lattice_core::EventId,
            _tool: &str,
            _params: serde_json::Value,
        ) -> Result<ExecutionResult, RouterError> {
            self.result.clone()
        }
    }

    #[tokio::test]
    async fn test_normal_flow_toolcall_then_final() {
        let session_id = SessionId::new_v4();

        let store = Arc::new(TestStore::new());
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::SessionCreated,
                parent_event_id: None,
            }],
        );

        let llm = Arc::new(TestLLM::new(Decision::FinalAnswer {
            answer: "done".to_string(),
        }));
        let router = Arc::new(TestRouter::new(Ok(ExecutionResult {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: 0,
        })));

        let control_loop = crate::ControlLoop::new(store, llm, router);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "done");
    }

    #[tokio::test]
    async fn test_thinking_flow() {
        let session_id = SessionId::new_v4();

        let store = Arc::new(TestStore::new());
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::SessionCreated,
                parent_event_id: None,
            }],
        );

        let llm = Arc::new(TestLLM::new(Decision::FinalAnswer {
            answer: "answer".to_string(),
        }));
        let router = Arc::new(TestRouter::new(Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        })));

        let control_loop = crate::ControlLoop::new(store, llm, router);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "answer");
    }

    #[tokio::test]
    async fn test_tool_call_failure_continues() {
        let session_id = SessionId::new_v4();

        let store = Arc::new(TestStore::new());
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::SessionCreated,
                parent_event_id: None,
            }],
        );

        let llm = Arc::new(TestLLM::new(Decision::FinalAnswer {
            answer: "recovered".to_string(),
        }));
        let router = Arc::new(TestRouter::new(Err(RouterError::ExecutionFailed(
            "boom".to_string(),
        ))));

        let control_loop = crate::ControlLoop::new(store, llm, router);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "recovered");
    }

    #[tokio::test]
    async fn test_max_iterations_protection() {
        let session_id = SessionId::new_v4();

        let store = Arc::new(TestStore::new());
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::SessionCreated,
                parent_event_id: None,
            }],
        );

        // LLM always returns Thinking — loop will hit max iterations.
        let llm = Arc::new(TestLLM::new(Decision::Thinking {
            reasoning: "loop".to_string(),
        }));
        let router = Arc::new(TestRouter::new(Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        })));

        let control_loop =
            crate::ControlLoop::with_options(store, llm, router, vec![], "prompt".to_string(), 3);

        let result = control_loop.run(session_id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max iterations"));
    }
}
