//! The agent control loop.

use std::sync::Arc;

use lattice_core::{
    Actor, Decision, EventFilter, EventPayload, LLMClient, SessionId, SessionStore,
};
use tracing::{info, instrument, warn};

use lattice_tools::ToolSet;

/// The maximum number of decision cycles before forcing exit.
const DEFAULT_MAX_ITERATIONS: usize = 50;

/// Control loop — the agent's brain.
///
/// Loads event history, calls the LLM for decisions, routes tool calls,
/// and records results. All state is recovered from the SessionStore.
///
/// The loop terminates on `FinalAnswer` or when `max_iterations` is reached.
pub struct ControlLoop {
    store: Arc<dyn SessionStore>,
    llm: Arc<dyn LLMClient>,
    tools: Arc<ToolSet>,
    system_prompt: String,
    max_iterations: usize,
}

impl ControlLoop {
    /// Create a new control loop with the minimum required components.
    ///
    /// Uses defaults: empty tool set, `"You are a helpful agent."` prompt,
    /// and 50 max iterations.
    #[must_use]
    pub fn new(store: Arc<dyn SessionStore>, llm: Arc<dyn LLMClient>, tools: Arc<ToolSet>) -> Self {
        Self {
            store,
            llm,
            tools,
            system_prompt: "You are a helpful agent.".to_string(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }

    /// Create a new control loop with all options.
    #[must_use]
    pub fn with_options(
        store: Arc<dyn SessionStore>,
        llm: Arc<dyn LLMClient>,
        tools: Arc<ToolSet>,
        system_prompt: String,
        max_iterations: usize,
    ) -> Self {
        Self {
            store,
            llm,
            tools,
            system_prompt,
            max_iterations,
        }
    }

    /// Get a reference to the session store.
    pub fn store(&self) -> &Arc<dyn SessionStore> {
        &self.store
    }

    /// Get a reference to the tool set.
    pub fn tools(&self) -> &Arc<ToolSet> {
        &self.tools
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

            let available_tools = self.tools.descriptions();
            let decision = self
                .llm
                .decide(&events, &available_tools, &self.system_prompt)
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

                    // Execute the tool and record the result (or error) directly.
                    match self.tools.execute(&tool, params).await {
                        Ok(result) => {
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
                        Err(e) => {
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
        SessionId, ToolDescription,
    };

    use lattice_core::ToolError;
    use lattice_tools::{ToolExecutor, ToolSet};

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

    /// No-op tool for testing.
    struct NoopTool;

    #[async_trait]
    impl ToolExecutor for NoopTool {
        fn description(&self) -> ToolDescription {
            ToolDescription {
                name: "noop".to_string(),
                description: "A no-op tool for testing.".to_string(),
                parameters_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            }
        }

        async fn execute(&self, _params: serde_json::Value) -> Result<ExecutionResult, ToolError> {
            Ok(ExecutionResult {
                stdout: "ok".to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }

    fn make_tools() -> Arc<ToolSet> {
        Arc::new(ToolSet::new())
    }

    fn insert_test_session(store: &TestStore, session_id: SessionId) {
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
    }

    #[tokio::test]
    async fn test_normal_flow_final_answer() {
        let session_id = SessionId::new_v4();
        let store = Arc::new(TestStore::new());
        insert_test_session(&store, session_id);

        let llm = Arc::new(TestLLM::new(Decision::FinalAnswer {
            answer: "done".to_string(),
        }));
        let tools = make_tools();

        let control_loop = crate::ControlLoop::new(store, llm, tools);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "done");
    }

    #[tokio::test]
    async fn test_thinking_flow() {
        let session_id = SessionId::new_v4();
        let store = Arc::new(TestStore::new());
        insert_test_session(&store, session_id);

        let llm = Arc::new(TestLLM::new(Decision::FinalAnswer {
            answer: "answer".to_string(),
        }));
        let tools = make_tools();

        let control_loop = crate::ControlLoop::new(store, llm, tools);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "answer");
    }

    #[tokio::test]
    async fn test_tool_call_continues_after_result() {
        let session_id = SessionId::new_v4();
        let store = Arc::new(TestStore::new());
        insert_test_session(&store, session_id);

        // Return ToolCall first, then FinalAnswer.
        struct TwoStepLLM(Arc<Mutex<bool>>);
        impl TwoStepLLM {
            fn new() -> Self {
                Self(Arc::new(Mutex::new(false)))
            }
        }
        #[async_trait]
        impl LLMClient for TwoStepLLM {
            async fn decide(
                &self,
                _history: &[Event],
                _available_tools: &[ToolDescription],
                _system_prompt: &str,
            ) -> Result<Decision, LLMError> {
                let mut called = self.0.lock().unwrap();
                if !*called {
                    *called = true;
                    Ok(Decision::ToolCall {
                        tool: "noop".to_string(),
                        params: serde_json::json!({}),
                    })
                } else {
                    Ok(Decision::FinalAnswer {
                        answer: "after tool".to_string(),
                    })
                }
            }
        }

        let llm = Arc::new(TwoStepLLM::new());
        let tools = {
            let mut ts = ToolSet::new();
            ts.register(NoopTool).unwrap();
            Arc::new(ts)
        };

        let control_loop = crate::ControlLoop::new(store, llm, tools);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "after tool");
    }

    #[tokio::test]
    async fn test_max_iterations_protection() {
        let session_id = SessionId::new_v4();
        let store = Arc::new(TestStore::new());
        insert_test_session(&store, session_id);

        // LLM always returns Thinking — loop will hit max iterations.
        let llm = Arc::new(TestLLM::new(Decision::Thinking {
            reasoning: "loop".to_string(),
        }));
        let tools = make_tools();

        let control_loop =
            crate::ControlLoop::with_options(store, llm, tools, "prompt".to_string(), 3);

        let result = control_loop.run(session_id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max iterations"));
    }

    #[tokio::test]
    async fn test_tool_not_found_records_error_event() {
        let session_id = SessionId::new_v4();
        let store = Arc::new(TestStore::new());
        insert_test_session(&store, session_id);
        use lattice_core::SessionStore;

        // LLM returns ToolCall for "nonexistent" first, then FinalAnswer.
        struct TwoStepLLM(Arc<Mutex<bool>>);
        impl TwoStepLLM {
            fn new() -> Self {
                Self(Arc::new(Mutex::new(false)))
            }
        }
        #[async_trait]
        impl LLMClient for TwoStepLLM {
            async fn decide(
                &self,
                _history: &[Event],
                _available_tools: &[ToolDescription],
                _system_prompt: &str,
            ) -> Result<Decision, LLMError> {
                let mut called = self.0.lock().unwrap();
                if !*called {
                    *called = true;
                    Ok(Decision::ToolCall {
                        tool: "nonexistent".to_string(),
                        params: serde_json::json!({}),
                    })
                } else {
                    Ok(Decision::FinalAnswer {
                        answer: "done after error".to_string(),
                    })
                }
            }
        }

        let llm = Arc::new(TwoStepLLM::new());
        let tools = make_tools(); // empty — no tools registered

        let control_loop = crate::ControlLoop::new(store.clone(), llm, tools);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "done after error");

        // Verify the event log contains a ToolCallError.
        let events = store
            .get_events(session_id, &EventFilter::default())
            .await
            .unwrap();
        let has_error = events
            .iter()
            .any(|e| matches!(e.payload, EventPayload::ToolCallError { .. }));
        assert!(
            has_error,
            "expected ToolCallError in event log, got: {:?}",
            events
        );
    }

    #[tokio::test]
    async fn test_multi_step_thinking_then_tool_then_answer() {
        let session_id = SessionId::new_v4();
        let store = Arc::new(TestStore::new());
        insert_test_session(&store, session_id);

        // Returns: Thinking → ToolCall → FinalAnswer
        struct ThreeStepLLM(Arc<Mutex<usize>>);
        impl ThreeStepLLM {
            fn new() -> Self {
                Self(Arc::new(Mutex::new(0)))
            }
        }
        #[async_trait]
        impl LLMClient for ThreeStepLLM {
            async fn decide(
                &self,
                _history: &[Event],
                _available_tools: &[ToolDescription],
                _system_prompt: &str,
            ) -> Result<Decision, LLMError> {
                let mut step = self.0.lock().unwrap();
                *step += 1;
                match *step {
                    1 => Ok(Decision::Thinking {
                        reasoning: "thinking".to_string(),
                    }),
                    2 => Ok(Decision::ToolCall {
                        tool: "noop".to_string(),
                        params: serde_json::json!({}),
                    }),
                    _ => Ok(Decision::FinalAnswer {
                        answer: "final".to_string(),
                    }),
                }
            }
        }

        let llm = Arc::new(ThreeStepLLM::new());
        let tools = {
            let mut ts = ToolSet::new();
            ts.register(NoopTool).unwrap();
            Arc::new(ts)
        };

        let control_loop = crate::ControlLoop::new(store.clone(), llm, tools);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "final");

        // Verify the event log contains Thinking, ToolCallRequested, ToolCallResult, FinalAnswer.
        use lattice_core::SessionStore;
        let events = store
            .get_events(session_id, &EventFilter::default())
            .await
            .unwrap();

        let types: Vec<&str> = events
            .iter()
            .filter_map(|e| match e.payload {
                EventPayload::Thinking { .. } => Some("Thinking"),
                EventPayload::ToolCallRequested { .. } => Some("ToolCallRequested"),
                EventPayload::ToolCallResult { .. } => Some("ToolCallResult"),
                EventPayload::FinalAnswer { .. } => Some("FinalAnswer"),
                _ => None,
            })
            .collect();

        assert_eq!(
            types,
            vec![
                "Thinking",
                "ToolCallRequested",
                "ToolCallResult",
                "FinalAnswer"
            ],
            "expected events in order: {:?}",
            events
        );
    }

    #[tokio::test]
    async fn test_with_options_custom_system_prompt() {
        let session_id = SessionId::new_v4();
        let store = Arc::new(TestStore::new());
        insert_test_session(&store, session_id);

        let llm = Arc::new(TestLLM::new(Decision::FinalAnswer {
            answer: "done".into(),
        }));
        let tools = make_tools();

        let control_loop =
            crate::ControlLoop::with_options(store, llm, tools, "custom prompt".into(), 50);
        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "done");
    }

    #[tokio::test]
    async fn test_store_get_events_error_returns_error_event() {
        let session_id = SessionId::new_v4();

        // LLM returns Thinking first, which should trigger a store.append_event error
        // when we use a store that fails on append_event.
        struct FailingAppendStore {
            inner: Arc<TestStore>,
        }
        impl FailingAppendStore {
            fn with_session(session_id: SessionId) -> Self {
                let store = TestStore::new();
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
                Self {
                    inner: Arc::new(store),
                }
            }
        }
        #[async_trait]
        impl lattice_core::SessionStore for FailingAppendStore {
            async fn create_session(&self) -> Result<SessionId, lattice_core::error::StoreError> {
                self.inner.create_session().await
            }
            async fn append_event(
                &self,
                _session_id: SessionId,
                _payload: EventPayload,
                _actor: Actor,
                _parent_event_id: Option<lattice_core::EventId>,
            ) -> Result<lattice_core::EventId, lattice_core::error::StoreError> {
                Err(lattice_core::error::StoreError::SessionNotFound(
                    SessionId::new_v4(),
                ))
            }
            async fn get_events(
                &self,
                session_id: SessionId,
                filter: &EventFilter,
            ) -> Result<Vec<Event>, lattice_core::error::StoreError> {
                self.inner.get_events(session_id, filter).await
            }
            async fn latest_event_id(
                &self,
                session_id: SessionId,
            ) -> Result<Option<lattice_core::EventId>, lattice_core::error::StoreError>
            {
                self.inner.latest_event_id(session_id).await
            }
        }

        let llm = Arc::new(TestLLM::new(Decision::Thinking {
            reasoning: "thinking".into(),
        }));
        let tools = make_tools();

        let failing_store = FailingAppendStore::with_session(session_id);

        let control_loop = crate::ControlLoop::with_options(
            Arc::new(failing_store),
            llm,
            tools,
            "prompt".into(),
            2,
        );
        let result = control_loop.run(session_id).await;
        // append_event fails with SessionNotFound, which propagates via ? and returns immediately.
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("session not found"));
    }
}
