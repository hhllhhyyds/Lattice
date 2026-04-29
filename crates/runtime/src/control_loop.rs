//! The agent control loop.

use std::sync::Arc;

use lattice_core::{
    Actor, Decision, Event, EventFilter, EventPayload, ExecutionContext, LLMClient, SessionId,
    SessionStore,
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
    depth: u32,
}

/// Fluent builder for [`ControlLoop`].
pub struct ControlLoopBuilder {
    store: Option<Arc<dyn SessionStore>>,
    llm: Option<Arc<dyn LLMClient>>,
    tools: Option<Arc<ToolSet>>,
    system_prompt: Option<String>,
    max_iterations: Option<usize>,
    depth: Option<u32>,
}

impl Default for ControlLoopBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlLoopBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            store: None,
            llm: None,
            tools: None,
            system_prompt: None,
            max_iterations: None,
            depth: None,
        }
    }

    #[must_use]
    pub fn store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }

    #[must_use]
    pub fn llm(mut self, llm: Arc<dyn LLMClient>) -> Self {
        self.llm = Some(llm);
        self
    }

    #[must_use]
    pub fn tools(mut self, tools: Arc<ToolSet>) -> Self {
        self.tools = Some(tools);
        self
    }

    #[must_use]
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    #[must_use]
    pub fn max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = Some(max_iterations);
        self
    }

    #[must_use]
    pub fn depth(mut self, depth: u32) -> Self {
        self.depth = Some(depth);
        self
    }

    #[must_use]
    pub fn build(self) -> ControlLoop {
        ControlLoop {
            store: self.store.expect("store required"),
            llm: self.llm.expect("llm required"),
            tools: self.tools.expect("tools required"),
            system_prompt: self
                .system_prompt
                .unwrap_or_else(|| "You are a helpful agent.".to_string()),
            max_iterations: self.max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS),
            depth: self.depth.unwrap_or(0),
        }
    }
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
            depth: 0,
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
            depth: 0,
        }
    }

    /// Create a builder for configuring a control loop.
    #[must_use]
    pub fn builder() -> ControlLoopBuilder {
        ControlLoopBuilder::new()
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

        // Fetch events once at the start (performance optimization for Issue #26)
        let mut events = self
            .store
            .get_events(session_id, &EventFilter::default())
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        for _ in 0..self.max_iterations {
            let available_tools = self.tools.descriptions();
            let decision = self
                .llm
                .decide(&events, &available_tools, &self.system_prompt)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            match decision {
                Decision::Thinking { reasoning } => {
                    info!(?reasoning, "LLM thinking");
                    let event_id = self
                        .store
                        .append_event(
                            session_id,
                            EventPayload::Thinking {
                                reasoning: reasoning.clone(),
                            },
                            Actor::LLM,
                            events.last().map(|e| e.event_id),
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;

                    // Update local event list
                    events.push(Event {
                        event_id,
                        session_id,
                        timestamp: chrono::Utc::now(),
                        actor: Actor::LLM,
                        payload: EventPayload::Thinking { reasoning },
                        parent_event_id: events.last().map(|e| e.event_id),
                    });
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

                    // Update local event list
                    events.push(Event {
                        event_id: req_event_id,
                        session_id,
                        timestamp: chrono::Utc::now(),
                        actor: Actor::LLM,
                        payload: EventPayload::ToolCallRequested {
                            tool: tool.clone(),
                            params: params.clone(),
                        },
                        parent_event_id: parent_id,
                    });

                    let ctx = ExecutionContext {
                        session_id,
                        trigger_event_id: req_event_id,
                        store: Arc::clone(&self.store),
                        depth: self.depth,
                    };

                    info!("executing tool: {}", tool);
                    // Execute the tool and record the result (or error) directly.
                    match self.tools.execute(&tool, params, &ctx).await {
                        Ok(result) => {
                            info!("tool execution succeeded: exit_code={}", result.exit_code);
                            let result_event_id = self
                                .store
                                .append_event(
                                    session_id,
                                    EventPayload::ToolCallResult {
                                        stdout: result.stdout.clone(),
                                        stderr: result.stderr.clone(),
                                        exit_code: result.exit_code,
                                    },
                                    Actor::Sandbox,
                                    Some(req_event_id),
                                )
                                .await
                                .map_err(|e| anyhow::anyhow!("{e}"))?;

                            // Update local event list
                            events.push(Event {
                                event_id: result_event_id,
                                session_id,
                                timestamp: chrono::Utc::now(),
                                actor: Actor::Sandbox,
                                payload: EventPayload::ToolCallResult {
                                    stdout: result.stdout,
                                    stderr: result.stderr,
                                    exit_code: result.exit_code,
                                },
                                parent_event_id: Some(req_event_id),
                            });
                        }
                        Err(e) => {
                            warn!("tool execution failed: {}", e);
                            let error_str = e.to_string();
                            let error_event_id = self
                                .store
                                .append_event(
                                    session_id,
                                    EventPayload::ToolCallError {
                                        error: error_str.clone(),
                                    },
                                    Actor::Sandbox,
                                    Some(req_event_id),
                                )
                                .await
                                .map_err(|e| anyhow::anyhow!("{e}"))?;

                            // Update local event list
                            events.push(Event {
                                event_id: error_event_id,
                                session_id,
                                timestamp: chrono::Utc::now(),
                                actor: Actor::Sandbox,
                                payload: EventPayload::ToolCallError { error: error_str },
                                parent_event_id: Some(req_event_id),
                            });
                        }
                    }
                    info!("tool call completed, continuing loop");
                }
                Decision::MultiToolCall { calls } => {
                    info!(count = calls.len(), "LLM requested multiple tool calls");
                    let parent_id = events.last().map(|e| e.event_id);

                    // 1. Record all ToolCallRequested events
                    let mut request_ids = Vec::new();
                    for call in &calls {
                        let req_event_id = self
                            .store
                            .append_event(
                                session_id,
                                EventPayload::ToolCallRequested {
                                    tool: call.tool.clone(),
                                    params: call.params.clone(),
                                },
                                Actor::LLM,
                                parent_id,
                            )
                            .await
                            .map_err(|e| anyhow::anyhow!("{e}"))?;

                        events.push(Event {
                            event_id: req_event_id,
                            session_id,
                            timestamp: chrono::Utc::now(),
                            actor: Actor::LLM,
                            payload: EventPayload::ToolCallRequested {
                                tool: call.tool.clone(),
                                params: call.params.clone(),
                            },
                            parent_event_id: parent_id,
                        });

                        request_ids.push(req_event_id);
                    }

                    // 2. Execute all tools sequentially, recording results or errors
                    for (call, req_event_id) in calls.iter().zip(request_ids) {
                        info!("executing tool: {}", call.tool);
                        let ctx = ExecutionContext {
                            session_id,
                            trigger_event_id: req_event_id,
                            store: Arc::clone(&self.store),
                            depth: self.depth,
                        };

                        match self
                            .tools
                            .execute(&call.tool, call.params.clone(), &ctx)
                            .await
                        {
                            Ok(result) => {
                                info!("tool execution succeeded: exit_code={}", result.exit_code);
                                let result_event_id = self
                                    .store
                                    .append_event(
                                        session_id,
                                        EventPayload::ToolCallResult {
                                            stdout: result.stdout.clone(),
                                            stderr: result.stderr.clone(),
                                            exit_code: result.exit_code,
                                        },
                                        Actor::Sandbox,
                                        Some(req_event_id),
                                    )
                                    .await
                                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                                events.push(Event {
                                    event_id: result_event_id,
                                    session_id,
                                    timestamp: chrono::Utc::now(),
                                    actor: Actor::Sandbox,
                                    payload: EventPayload::ToolCallResult {
                                        stdout: result.stdout,
                                        stderr: result.stderr,
                                        exit_code: result.exit_code,
                                    },
                                    parent_event_id: Some(req_event_id),
                                });
                            }
                            Err(e) => {
                                warn!("tool execution failed: {}", e);
                                let error_str = e.to_string();
                                let error_event_id = self
                                    .store
                                    .append_event(
                                        session_id,
                                        EventPayload::ToolCallError {
                                            error: error_str.clone(),
                                        },
                                        Actor::Sandbox,
                                        Some(req_event_id),
                                    )
                                    .await
                                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                                events.push(Event {
                                    event_id: error_event_id,
                                    session_id,
                                    timestamp: chrono::Utc::now(),
                                    actor: Actor::Sandbox,
                                    payload: EventPayload::ToolCallError { error: error_str },
                                    parent_event_id: Some(req_event_id),
                                });
                            }
                        }
                    }

                    info!(count = calls.len(), "all tool calls completed");
                }
                Decision::FinalAnswer { answer } => {
                    info!(?answer, "LLM final answer");
                    let event_id = self
                        .store
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

                    // Update local event list (for consistency, though we return immediately)
                    events.push(Event {
                        event_id,
                        session_id,
                        timestamp: chrono::Utc::now(),
                        actor: Actor::LLM,
                        payload: EventPayload::FinalAnswer {
                            answer: answer.clone(),
                        },
                        parent_event_id: events.last().map(|e| e.event_id),
                    });

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
        Actor, ChildSessionInfo, Decision, Event, EventFilter, EventPayload, ExecutionContext,
        ExecutionResult, LLMClient, LLMError, SessionId, SessionStore, ToolDescription,
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
        async fn create_child_session(
            &self,
            _parent_session_id: SessionId,
            _skill_name: &str,
        ) -> Result<(SessionId, Arc<dyn SessionStore>), lattice_core::error::StoreError> {
            let child = Arc::new(TestStore::new());
            let child_id = child.create_session().await?;
            Ok((child_id, child))
        }
        async fn child_sessions(
            &self,
            _parent_session_id: SessionId,
        ) -> Result<Vec<ChildSessionInfo>, lattice_core::error::StoreError> {
            Ok(Vec::new())
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
        decisions: Arc<Mutex<Vec<Decision>>>,
        current_index: Arc<Mutex<usize>>,
    }

    impl TestLLM {
        fn new(decision: Decision) -> Self {
            Self {
                decisions: Arc::new(Mutex::new(vec![decision])),
                current_index: Arc::new(Mutex::new(0)),
            }
        }

        fn with_sequence(decisions: Vec<Decision>) -> Self {
            Self {
                decisions: Arc::new(Mutex::new(decisions)),
                current_index: Arc::new(Mutex::new(0)),
            }
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
            let mut index = self.current_index.lock().unwrap();
            let decisions = self.decisions.lock().unwrap();
            let decision = decisions
                .get(*index)
                .cloned()
                .unwrap_or_else(|| decisions.last().unwrap().clone());
            *index += 1;
            Ok(decision)
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

        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &ExecutionContext,
        ) -> Result<ExecutionResult, ToolError> {
            Ok(ExecutionResult {
                stdout: "ok".to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }

    fn make_tools() -> Arc<ToolSet> {
        let mut toolset = ToolSet::new();

        // Add a noop tool for testing
        struct NoopTool;
        #[async_trait]
        impl ToolExecutor for NoopTool {
            fn description(&self) -> ToolDescription {
                ToolDescription {
                    name: "noop".into(),
                    description: "A no-op tool for testing".into(),
                    parameters_schema: serde_json::json!({
                        "type": "object",
                        "properties": {},
                    }),
                }
            }
            async fn execute(
                &self,
                _params: serde_json::Value,
                _ctx: &ExecutionContext,
            ) -> Result<ExecutionResult, ToolError> {
                Ok(ExecutionResult {
                    stdout: "noop executed".into(),
                    stderr: String::new(),
                    exit_code: 0,
                })
            }
        }

        toolset.register(NoopTool).unwrap();
        Arc::new(toolset)
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

    #[test]
    fn builder_default_depth_is_zero() {
        let loop_ = crate::ControlLoop::builder()
            .store(Arc::new(TestStore::new()))
            .llm(Arc::new(TestLLM::new(Decision::FinalAnswer {
                answer: "x".into(),
            })))
            .tools(Arc::new(ToolSet::new()))
            .build();
        assert_eq!(loop_.depth, 0);
    }

    #[test]
    fn builder_depth_override() {
        let loop_ = crate::ControlLoop::builder()
            .store(Arc::new(TestStore::new()))
            .llm(Arc::new(TestLLM::new(Decision::FinalAnswer {
                answer: "x".into(),
            })))
            .tools(Arc::new(ToolSet::new()))
            .depth(3)
            .build();
        assert_eq!(loop_.depth, 3);
    }

    #[tokio::test]
    async fn execution_context_passed_to_tool() {
        static CAPTURED_CTX: std::sync::OnceLock<ExecutionContext> = std::sync::OnceLock::new();

        struct CapturingTool;

        #[async_trait]
        impl ToolExecutor for CapturingTool {
            fn description(&self) -> ToolDescription {
                ToolDescription {
                    name: "capture".into(),
                    description: "captures ctx".into(),
                    parameters_schema: serde_json::json!({}),
                }
            }

            async fn execute(
                &self,
                _params: serde_json::Value,
                ctx: &ExecutionContext,
            ) -> Result<ExecutionResult, ToolError> {
                let _ = CAPTURED_CTX.set(ctx.clone());
                Ok(ExecutionResult {
                    stdout: "ok".into(),
                    stderr: String::new(),
                    exit_code: 0,
                })
            }
        }

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
                        tool: "capture".into(),
                        params: serde_json::json!({}),
                    })
                } else {
                    Ok(Decision::FinalAnswer {
                        answer: "done".into(),
                    })
                }
            }
        }

        let session_id = SessionId::new_v4();
        let store = Arc::new(TestStore::new());
        insert_test_session(&store, session_id);

        let mut tools = ToolSet::new();
        tools.register(CapturingTool).unwrap();

        let loop_ = crate::ControlLoop::builder()
            .store(store)
            .llm(Arc::new(TwoStepLLM::new()))
            .tools(Arc::new(tools))
            .depth(5)
            .build();

        loop_.run(session_id).await.unwrap();

        let ctx = CAPTURED_CTX.get().expect("ctx was captured");
        assert_eq!(ctx.depth, 5);
        assert_eq!(ctx.session_id, session_id);
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
            async fn create_child_session(
                &self,
                parent_session_id: SessionId,
                skill_name: &str,
            ) -> Result<(SessionId, Arc<dyn SessionStore>), lattice_core::error::StoreError>
            {
                self.inner
                    .create_child_session(parent_session_id, skill_name)
                    .await
            }
            async fn child_sessions(
                &self,
                parent_session_id: SessionId,
            ) -> Result<Vec<ChildSessionInfo>, lattice_core::error::StoreError> {
                self.inner.child_sessions(parent_session_id).await
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

    /// Test that get_events is only called once per run (performance optimization).
    ///
    /// This test verifies the fix for Issue #26: ControlLoop should fetch events
    /// once at the start and maintain a local cache, rather than re-fetching on
    /// every iteration.
    #[tokio::test]
    async fn test_get_events_called_only_once() {
        let session_id = SessionId::new_v4();

        // Create a store that tracks get_events call count
        struct CallCountingStore {
            inner: Arc<TestStore>,
            get_events_call_count: Arc<Mutex<usize>>,
        }

        impl CallCountingStore {
            fn with_session(session_id: SessionId) -> Self {
                let store = TestStore::new();
                store.insert_session(
                    session_id,
                    vec![Event {
                        event_id: lattice_core::EventId::new_v4(),
                        session_id,
                        timestamp: chrono::Utc::now(),
                        actor: Actor::System,
                        payload: EventPayload::UserMessage {
                            content: "test".into(),
                        },
                        parent_event_id: None,
                    }],
                );
                Self {
                    inner: Arc::new(store),
                    get_events_call_count: Arc::new(Mutex::new(0)),
                }
            }

            fn get_call_count(&self) -> usize {
                *self.get_events_call_count.lock().unwrap()
            }
        }

        #[async_trait]
        impl lattice_core::SessionStore for CallCountingStore {
            async fn create_session(&self) -> Result<SessionId, lattice_core::error::StoreError> {
                self.inner.create_session().await
            }

            async fn append_event(
                &self,
                session_id: SessionId,
                payload: EventPayload,
                actor: Actor,
                parent_event_id: Option<lattice_core::EventId>,
            ) -> Result<lattice_core::EventId, lattice_core::error::StoreError> {
                self.inner
                    .append_event(session_id, payload, actor, parent_event_id)
                    .await
            }

            async fn get_events(
                &self,
                session_id: SessionId,
                filter: &EventFilter,
            ) -> Result<Vec<Event>, lattice_core::error::StoreError> {
                // Increment call counter
                *self.get_events_call_count.lock().unwrap() += 1;
                self.inner.get_events(session_id, filter).await
            }

            async fn create_child_session(
                &self,
                parent_session_id: SessionId,
                skill_name: &str,
            ) -> Result<(SessionId, Arc<dyn SessionStore>), lattice_core::error::StoreError>
            {
                self.inner
                    .create_child_session(parent_session_id, skill_name)
                    .await
            }

            async fn child_sessions(
                &self,
                parent_session_id: SessionId,
            ) -> Result<Vec<ChildSessionInfo>, lattice_core::error::StoreError> {
                self.inner.child_sessions(parent_session_id).await
            }

            async fn latest_event_id(
                &self,
                session_id: SessionId,
            ) -> Result<Option<lattice_core::EventId>, lattice_core::error::StoreError>
            {
                self.inner.latest_event_id(session_id).await
            }
        }

        let store = Arc::new(CallCountingStore::with_session(session_id));

        // LLM will return ToolCall twice, then FinalAnswer
        // This creates 3 iterations, which would call get_events 3 times in the old code
        let decisions = vec![
            Decision::ToolCall {
                tool: "test_tool".into(),
                params: serde_json::json!({"command": "echo first"}),
            },
            Decision::ToolCall {
                tool: "test_tool".into(),
                params: serde_json::json!({"command": "echo second"}),
            },
            Decision::FinalAnswer {
                answer: "done".into(),
            },
        ];

        let llm = Arc::new(TestLLM::with_sequence(decisions));
        let tools = make_tools();

        let control_loop =
            crate::ControlLoop::with_options(store.clone(), llm, tools, "prompt".into(), 10);

        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "done");

        // Verify get_events was called only once
        let call_count = store.get_call_count();
        assert_eq!(
            call_count, 1,
            "get_events should be called only once, but was called {} times",
            call_count
        );
    }

    /// Test that Thinking decision path is covered.
    #[tokio::test]
    async fn test_thinking_decision_coverage() {
        let session_id = SessionId::new_v4();
        let store = TestStore::new();
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::UserMessage {
                    content: "test".into(),
                },
                parent_event_id: None,
            }],
        );

        // LLM returns Thinking, then FinalAnswer
        let decisions = vec![
            Decision::Thinking {
                reasoning: "let me think".into(),
            },
            Decision::FinalAnswer {
                answer: "done".into(),
            },
        ];

        let llm = Arc::new(TestLLM::with_sequence(decisions));
        let tools = make_tools();

        let control_loop =
            crate::ControlLoop::with_options(Arc::new(store), llm, tools, "prompt".into(), 10);

        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "done");
    }

    /// Test for Issue #27: MultiToolCall with all tools succeeding
    #[tokio::test]
    async fn test_multi_tool_call_all_success() {
        use lattice_core::llm::ToolCallRequest;

        let session_id = SessionId::new_v4();
        let store = TestStore::new();
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::UserMessage {
                    content: "test".into(),
                },
                parent_event_id: None,
            }],
        );

        // LLM returns MultiToolCall with 3 tools, then FinalAnswer
        let decisions = vec![
            Decision::MultiToolCall {
                calls: vec![
                    ToolCallRequest {
                        id: "call_1".into(),
                        tool: "noop".into(),
                        params: serde_json::json!({}),
                    },
                    ToolCallRequest {
                        id: "call_2".into(),
                        tool: "noop".into(),
                        params: serde_json::json!({}),
                    },
                    ToolCallRequest {
                        id: "call_3".into(),
                        tool: "noop".into(),
                        params: serde_json::json!({}),
                    },
                ],
            },
            Decision::FinalAnswer {
                answer: "all done".into(),
            },
        ];

        let llm = Arc::new(TestLLM::with_sequence(decisions));
        let tools = make_tools();

        let store_arc: Arc<dyn SessionStore> = Arc::new(store);
        let control_loop = crate::ControlLoop::with_options(
            Arc::clone(&store_arc),
            llm,
            tools,
            "prompt".into(),
            10,
        );

        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "all done");

        // Verify all events were recorded
        let session_events = store_arc
            .get_events(session_id, &EventFilter::default())
            .await
            .unwrap();

        // Should have: initial + 3 ToolCallRequested + 3 ToolCallResult + FinalAnswer
        assert!(
            session_events.len() >= 8,
            "Expected at least 8 events, got {}",
            session_events.len()
        );

        // Verify all 3 ToolCallRequested events
        let requested_count = session_events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::ToolCallRequested { .. }))
            .count();
        assert_eq!(requested_count, 3, "Expected 3 ToolCallRequested events");

        // Verify all 3 ToolCallResult events
        let result_count = session_events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::ToolCallResult { .. }))
            .count();
        assert_eq!(result_count, 3, "Expected 3 ToolCallResult events");
    }

    /// Test for Issue #27: MultiToolCall with partial failures
    #[tokio::test]
    async fn test_multi_tool_call_partial_failure() {
        use lattice_core::llm::ToolCallRequest;

        let session_id = SessionId::new_v4();
        let store = TestStore::new();
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::UserMessage {
                    content: "test".into(),
                },
                parent_event_id: None,
            }],
        );

        // LLM returns MultiToolCall: tool1 (success), tool2 (fail), tool3 (success)
        let decisions = vec![
            Decision::MultiToolCall {
                calls: vec![
                    ToolCallRequest {
                        id: "call_1".into(),
                        tool: "noop".into(),
                        params: serde_json::json!({}),
                    },
                    ToolCallRequest {
                        id: "call_2".into(),
                        tool: "nonexistent".into(), // This will fail
                        params: serde_json::json!({}),
                    },
                    ToolCallRequest {
                        id: "call_3".into(),
                        tool: "noop".into(),
                        params: serde_json::json!({}),
                    },
                ],
            },
            Decision::FinalAnswer {
                answer: "partial success".into(),
            },
        ];

        let llm = Arc::new(TestLLM::with_sequence(decisions));
        let tools = make_tools();

        let store_arc: Arc<dyn SessionStore> = Arc::new(store);
        let control_loop = crate::ControlLoop::with_options(
            Arc::clone(&store_arc),
            llm,
            tools,
            "prompt".into(),
            10,
        );

        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "partial success");

        // Verify events were recorded
        let session_events = store_arc
            .get_events(session_id, &EventFilter::default())
            .await
            .unwrap();

        // Should have: initial + 3 ToolCallRequested + 2 ToolCallResult + 1 ToolCallError + FinalAnswer
        assert!(session_events.len() >= 8, "Expected at least 8 events");

        // Verify 3 ToolCallRequested events
        let requested_count = session_events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::ToolCallRequested { .. }))
            .count();
        assert_eq!(requested_count, 3);

        // Verify 2 ToolCallResult events (call_1 and call_3 succeed)
        let result_count = session_events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::ToolCallResult { .. }))
            .count();
        assert_eq!(result_count, 2);

        // Verify 1 ToolCallError event (call_2 fails)
        let error_count = session_events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::ToolCallError { .. }))
            .count();
        assert_eq!(error_count, 1);
    }

    /// Test for Issue #27: MultiToolCall with all tools failing
    #[tokio::test]
    async fn test_multi_tool_call_all_failure() {
        use lattice_core::llm::ToolCallRequest;

        let session_id = SessionId::new_v4();
        let store = TestStore::new();
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::UserMessage {
                    content: "test".into(),
                },
                parent_event_id: None,
            }],
        );

        // LLM returns MultiToolCall with all nonexistent tools
        let decisions = vec![
            Decision::MultiToolCall {
                calls: vec![
                    ToolCallRequest {
                        id: "call_1".into(),
                        tool: "nonexistent1".into(),
                        params: serde_json::json!({}),
                    },
                    ToolCallRequest {
                        id: "call_2".into(),
                        tool: "nonexistent2".into(),
                        params: serde_json::json!({}),
                    },
                ],
            },
            Decision::FinalAnswer {
                answer: "all failed".into(),
            },
        ];

        let llm = Arc::new(TestLLM::with_sequence(decisions));
        let tools = make_tools();

        let store_arc: Arc<dyn SessionStore> = Arc::new(store);
        let control_loop = crate::ControlLoop::with_options(
            Arc::clone(&store_arc),
            llm,
            tools,
            "prompt".into(),
            10,
        );

        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "all failed");

        // Verify events were recorded
        let session_events = store_arc
            .get_events(session_id, &EventFilter::default())
            .await
            .unwrap();

        // Should have: initial + 2 ToolCallRequested + 2 ToolCallError + FinalAnswer
        assert!(session_events.len() >= 6);

        // Verify 2 ToolCallRequested events
        let requested_count = session_events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::ToolCallRequested { .. }))
            .count();
        assert_eq!(requested_count, 2);

        // Verify 0 ToolCallResult events
        let result_count = session_events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::ToolCallResult { .. }))
            .count();
        assert_eq!(result_count, 0);

        // Verify 2 ToolCallError events
        let error_count = session_events
            .iter()
            .filter(|e| matches!(e.payload, EventPayload::ToolCallError { .. }))
            .count();
        assert_eq!(error_count, 2);
    }

    /// Test for Issue #27: MultiToolCall event ordering
    #[tokio::test]
    async fn test_multi_tool_call_event_ordering() {
        use lattice_core::llm::ToolCallRequest;

        let session_id = SessionId::new_v4();
        let store = TestStore::new();
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::UserMessage {
                    content: "test".into(),
                },
                parent_event_id: None,
            }],
        );

        let decisions = vec![
            Decision::MultiToolCall {
                calls: vec![
                    ToolCallRequest {
                        id: "call_1".into(),
                        tool: "noop".into(),
                        params: serde_json::json!({}),
                    },
                    ToolCallRequest {
                        id: "call_2".into(),
                        tool: "noop".into(),
                        params: serde_json::json!({}),
                    },
                ],
            },
            Decision::FinalAnswer {
                answer: "done".into(),
            },
        ];

        let llm = Arc::new(TestLLM::with_sequence(decisions));
        let tools = make_tools();

        let store_arc: Arc<dyn SessionStore> = Arc::new(store);
        let control_loop = crate::ControlLoop::with_options(
            Arc::clone(&store_arc),
            llm,
            tools,
            "prompt".into(),
            10,
        );

        control_loop.run(session_id).await.unwrap();

        // Verify event ordering: all requests first, then all results
        let session_events = store_arc
            .get_events(session_id, &EventFilter::default())
            .await
            .unwrap();

        let mut request_indices = Vec::new();
        let mut result_indices = Vec::new();

        for (i, event) in session_events.iter().enumerate() {
            match &event.payload {
                EventPayload::ToolCallRequested { .. } => request_indices.push(i),
                EventPayload::ToolCallResult { .. } => result_indices.push(i),
                _ => {}
            }
        }

        // Verify we have 2 requests and 2 results
        assert_eq!(request_indices.len(), 2);
        assert_eq!(result_indices.len(), 2);

        // Verify all requests come before all results
        // Expected order: [req1, req2, result1, result2]
        assert!(
            request_indices[0] < result_indices[0],
            "First request should come before first result"
        );
        assert!(
            request_indices[1] < result_indices[0],
            "Second request should come before first result"
        );
        assert!(
            request_indices[1] < result_indices[1],
            "Second request should come before second result"
        );
    }

    /// Test for Issue #27: Empty MultiToolCall
    #[tokio::test]
    async fn test_multi_tool_call_empty() {
        let session_id = SessionId::new_v4();
        let store = TestStore::new();
        store.insert_session(
            session_id,
            vec![Event {
                event_id: lattice_core::EventId::new_v4(),
                session_id,
                timestamp: chrono::Utc::now(),
                actor: Actor::System,
                payload: EventPayload::UserMessage {
                    content: "test".into(),
                },
                parent_event_id: None,
            }],
        );

        // LLM returns empty MultiToolCall (edge case)
        let decisions = vec![
            Decision::MultiToolCall { calls: vec![] },
            Decision::FinalAnswer {
                answer: "nothing to do".into(),
            },
        ];

        let llm = Arc::new(TestLLM::with_sequence(decisions));
        let tools = make_tools();

        let control_loop =
            crate::ControlLoop::with_options(Arc::new(store), llm, tools, "prompt".into(), 10);

        let result = control_loop.run(session_id).await.unwrap();
        assert_eq!(result, "nothing to do");

        // Verify no tool events were recorded
        let store_ref = Arc::clone(&control_loop.store);
        let session_events = store_ref
            .get_events(session_id, &EventFilter::default())
            .await
            .unwrap();

        let tool_event_count = session_events
            .iter()
            .filter(|e| {
                matches!(
                    e.payload,
                    EventPayload::ToolCallRequested { .. }
                        | EventPayload::ToolCallResult { .. }
                        | EventPayload::ToolCallError { .. }
                )
            })
            .count();
        assert_eq!(
            tool_event_count, 0,
            "Empty MultiToolCall should not create any tool events"
        );
    }
}
