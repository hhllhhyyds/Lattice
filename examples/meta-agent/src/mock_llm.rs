//! Mock LLM for meta-agent example.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lattice::core::{Decision, Event, LLMClient, LLMError, ToolDescription};

/// Mock LLM that simulates a meta agent delegating to a skill.
pub struct MetaAgentMockLLM {
    step: Arc<Mutex<usize>>,
}

impl MetaAgentMockLLM {
    pub fn new() -> Self {
        Self {
            step: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LLMClient for MetaAgentMockLLM {
    async fn decide(
        &self,
        _history: &[Event],
        tools: &[ToolDescription],
        _system_prompt: &str,
    ) -> Result<Decision, LLMError> {
        let mut step = self.step.lock().unwrap();
        *step += 1;

        match *step {
            1 => {
                // Meta agent decides to delegate to skill
                // Check if skill tool is available
                let skill_tool = tools
                    .iter()
                    .find(|t| t.name.starts_with("skill:"))
                    .map(|t| t.name.clone());

                if let Some(skill_name) = skill_tool {
                    Ok(Decision::ToolCall {
                        tool: skill_name,
                        params: serde_json::json!({
                            "input": "Research Rust async runtimes"
                        }),
                    })
                } else {
                    // No skill available, return answer directly
                    Ok(Decision::FinalAnswer {
                        answer: "No skills available. Cannot perform research.".to_string(),
                    })
                }
            }
            2 => {
                // After skill returns, meta agent returns the result
                Ok(Decision::FinalAnswer {
                    answer: "Research completed. The skill found that Rust async runtimes \
                             (Tokio, async-std, Smol) continue to evolve with improved \
                             performance and ergonomics."
                        .to_string(),
                })
            }
            _ => Ok(Decision::FinalAnswer {
                answer: "Unexpected state".to_string(),
            }),
        }
    }
}
