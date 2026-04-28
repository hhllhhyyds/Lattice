//! LLM client types and the LLMClient trait.

use async_trait::async_trait;

use crate::error::LLMError;
use crate::{Event, ToolDescription};

/// A single tool call request within a multi-tool-call decision.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ToolCallRequest {
    /// Unique identifier for this tool call (from LLM provider).
    pub id: String,
    /// Name of the tool to invoke.
    pub tool: String,
    /// Tool parameters as JSON.
    pub params: serde_json::Value,
}

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
    /// LLM wants to invoke multiple tools sequentially.
    /// All tools will be executed regardless of individual failures.
    MultiToolCall {
        /// List of tool calls to execute in order.
        calls: Vec<ToolCallRequest>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_request_serialization() {
        let request = ToolCallRequest {
            id: "call_123".into(),
            tool: "bash".into(),
            params: serde_json::json!({"command": "ls"}),
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["id"], "call_123");
        assert_eq!(json["tool"], "bash");
        assert_eq!(json["params"]["command"], "ls");

        let deserialized: ToolCallRequest = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_decision_thinking_serialization() {
        let decision = Decision::Thinking {
            reasoning: "let me think".into(),
        };

        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["type"], "thinking");
        assert_eq!(json["reasoning"], "let me think");

        let deserialized: Decision = serde_json::from_value(json).unwrap();
        match deserialized {
            Decision::Thinking { reasoning } => assert_eq!(reasoning, "let me think"),
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn test_decision_tool_call_serialization() {
        let decision = Decision::ToolCall {
            tool: "bash".into(),
            params: serde_json::json!({"command": "pwd"}),
        };

        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["type"], "toolCall");
        assert_eq!(json["tool"], "bash");

        let deserialized: Decision = serde_json::from_value(json).unwrap();
        match deserialized {
            Decision::ToolCall { tool, params } => {
                assert_eq!(tool, "bash");
                assert_eq!(params["command"], "pwd");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn test_decision_multi_tool_call_serialization() {
        let decision = Decision::MultiToolCall {
            calls: vec![
                ToolCallRequest {
                    id: "call_1".into(),
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "ls"}),
                },
                ToolCallRequest {
                    id: "call_2".into(),
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "pwd"}),
                },
            ],
        };

        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["type"], "multiToolCall");
        assert_eq!(json["calls"].as_array().unwrap().len(), 2);
        assert_eq!(json["calls"][0]["id"], "call_1");
        assert_eq!(json["calls"][1]["id"], "call_2");

        let deserialized: Decision = serde_json::from_value(json).unwrap();
        match deserialized {
            Decision::MultiToolCall { calls } => {
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].id, "call_1");
                assert_eq!(calls[1].id, "call_2");
            }
            _ => panic!("expected MultiToolCall"),
        }
    }

    #[test]
    fn test_decision_final_answer_serialization() {
        let decision = Decision::FinalAnswer {
            answer: "done".into(),
        };

        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["type"], "finalAnswer");
        assert_eq!(json["answer"], "done");

        let deserialized: Decision = serde_json::from_value(json).unwrap();
        match deserialized {
            Decision::FinalAnswer { answer } => assert_eq!(answer, "done"),
            _ => panic!("expected FinalAnswer"),
        }
    }

    #[test]
    fn test_multi_tool_call_empty_calls() {
        let decision = Decision::MultiToolCall { calls: vec![] };

        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["type"], "multiToolCall");
        assert_eq!(json["calls"].as_array().unwrap().len(), 0);

        let deserialized: Decision = serde_json::from_value(json).unwrap();
        match deserialized {
            Decision::MultiToolCall { calls } => assert_eq!(calls.len(), 0),
            _ => panic!("expected MultiToolCall"),
        }
    }
}
