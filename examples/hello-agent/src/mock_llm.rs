//! Mock LLM client that returns hardcoded decisions in sequence.
//!
//! Built on `Mutex<VecDeque<Decision>>` so decisions are consumed in order
//! and the queue is transparent for inspection.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use lattice_core::{Decision, Event, LLMClient, LLMError, ToolDescription};

/// Mock LLM client that pops decisions from a queue in order.
#[derive(Debug)]
pub struct MockLLMClient {
    decisions: Mutex<VecDeque<Decision>>,
}

impl MockLLMClient {
    /// Create a new MockLLMClient with the given decision sequence.
    ///
    /// Decisions are consumed in order on each call to `decide()`.
    #[must_use]
    pub fn new(decisions: Vec<Decision>) -> Self {
        Self {
            decisions: Mutex::new(decisions.into()),
        }
    }

    /// Create the standard hello-agent decision sequence:
    /// 1. ToolCall to bash
    /// 2. FinalAnswer
    #[must_use]
    pub fn hello_agent_sequence() -> Self {
        Self::new(vec![
            Decision::ToolCall {
                tool: "bash".to_string(),
                params: serde_json::json!({ "command": "echo Hello from Lattice!" }),
            },
            Decision::FinalAnswer {
                answer: "命令执行成功，输出: Hello from Lattice!".to_string(),
            },
        ])
    }
}

#[async_trait]
impl LLMClient for MockLLMClient {
    async fn decide(
        &self,
        _history: &[Event],
        _available_tools: &[ToolDescription],
        _system_prompt: &str,
    ) -> Result<Decision, LLMError> {
        let mut queue = self.decisions.lock().unwrap();
        queue.pop_front().ok_or_else(|| {
            LLMError::InvalidResponse("mock LLM: decision queue exhausted".to_string())
        })
    }
}
