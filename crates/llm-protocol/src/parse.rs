//! Parse LLM responses into Lattice decisions.

use lattice_core::error::LLMError;
use lattice_core::llm::Decision;

use crate::message::ContentBlock;
use crate::response::LLMResponse;

/// Convert a provider-agnostic LLM response into a Lattice Decision.
///
/// # Rules
/// - `Text` → `FinalAnswer`
/// - `ToolUse` → `ToolCall`
/// - `Mixed` with any `ToolUse` → first `ToolCall`
/// - `Mixed` with only `Text` → `FinalAnswer` (concatenated)
/// - `Error` → `LLMError::InvalidResponse`
pub fn response_to_decision(response: LLMResponse) -> Result<Decision, LLMError> {
    match response {
        LLMResponse::Text { text } => Ok(Decision::FinalAnswer { answer: text }),

        LLMResponse::ToolUse { name, input, .. } => Ok(Decision::ToolCall {
            tool: name,
            params: input,
        }),

        LLMResponse::Mixed { blocks } => {
            // Look for the first ToolUse block.
            for block in &blocks {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    return Ok(Decision::ToolCall {
                        tool: name.clone(),
                        params: input.clone(),
                    });
                }
            }

            // No tool use — concatenate all text blocks into a final answer.
            let text: String = blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            if text.is_empty() {
                Err(LLMError::InvalidResponse(
                    "mixed response contained no text or tool_use blocks".into(),
                ))
            } else {
                Ok(Decision::FinalAnswer { answer: text })
            }
        }

        LLMResponse::Error { message } => Err(LLMError::InvalidResponse(message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_to_final_answer() {
        let resp = LLMResponse::Text {
            text: "done".into(),
        };
        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::FinalAnswer { answer } => assert_eq!(answer, "done"),
            _ => panic!("expected FinalAnswer"),
        }
    }

    #[test]
    fn test_tool_use_to_tool_call() {
        let resp = LLMResponse::ToolUse {
            id: "t1".into(),
            name: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
        };
        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::ToolCall { tool, params } => {
                assert_eq!(tool, "bash");
                assert_eq!(params, serde_json::json!({"command": "ls"}));
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn test_mixed_with_tool_use() {
        let resp = LLMResponse::Mixed {
            blocks: vec![
                ContentBlock::Text {
                    text: "thinking...".into(),
                },
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "pwd"}),
                },
            ],
        };
        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::ToolCall { tool, .. } => assert_eq!(tool, "bash"),
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn test_mixed_text_only() {
        let resp = LLMResponse::Mixed {
            blocks: vec![
                ContentBlock::Text {
                    text: "part 1".into(),
                },
                ContentBlock::Text {
                    text: "part 2".into(),
                },
            ],
        };
        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::FinalAnswer { answer } => assert_eq!(answer, "part 1\npart 2"),
            _ => panic!("expected FinalAnswer"),
        }
    }

    #[test]
    fn test_error_response() {
        let resp = LLMResponse::Error {
            message: "rate limited".into(),
        };
        let result = response_to_decision(resp);
        assert!(result.is_err());
    }

    #[test]
    fn test_mixed_empty_blocks() {
        let resp = LLMResponse::Mixed { blocks: vec![] };
        let result = response_to_decision(resp);
        assert!(result.is_err());
    }
}
