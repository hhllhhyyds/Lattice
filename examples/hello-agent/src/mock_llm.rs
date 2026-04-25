//! Mock LLM client for hello-agent example.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use lattice_core::{Decision, Event, LLMClient, LLMError, ToolDescription};

/// Mock LLM client that cycles through a hardcoded decision sequence.
pub struct MockLLMClient {
    step: AtomicUsize,
}

impl MockLLMClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            step: AtomicUsize::new(0),
        }
    }
}

impl Default for MockLLMClient {
    fn default() -> Self {
        Self::new()
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
        let step = self.step.fetch_add(1, Ordering::SeqCst);
        match step {
            0 => Ok(Decision::ToolCall {
                tool: "bash".to_string(),
                params: serde_json::json!({ "command": "echo 'hello from sandbox'" }),
            }),
            1 => Ok(Decision::FinalAnswer {
                answer: "Hello back from the agent!".to_string(),
            }),
            _ => Ok(Decision::FinalAnswer {
                answer: "done".to_string(),
            }),
        }
    }
}
