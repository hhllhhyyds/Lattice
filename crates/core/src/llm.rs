//! LLM client types and the LLMClient trait.

use async_trait::async_trait;

use crate::error::LLMError;
use crate::{Event, ToolDescription};

/// LLM decision types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Decision {
    /// LLM is thinking — continue the loop without side effects.
    Thinking {
        /// Reasoning text from the LLM.
        reasoning: String,
    },
    /// LLM wants to invoke a tool.
    ToolCall {
        /// Name of the tool to invoke.
        tool: String,
        /// Tool parameters as JSON.
        params: serde_json::Value,
    },
    /// LLM has produced a final answer — loop terminates.
    FinalAnswer {
        /// Answer text.
        answer: String,
    },
}

/// LLM client — decision making.
///
/// The client receives event history and available tools, and returns
/// the next decision to be executed by the control loop.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Decide the next action based on event history.
    async fn decide(
        &self,
        history: &[Event],
        available_tools: &[ToolDescription],
        system_prompt: &str,
    ) -> Result<Decision, LLMError>;
}
