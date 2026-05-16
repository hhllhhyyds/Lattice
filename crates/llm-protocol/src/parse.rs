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
/// - `Mixed` with single `ToolUse` → `ToolCall` (backward compatible)
/// - `Mixed` with multiple `ToolUse` → `MultiToolCall`
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
            // Collect all ToolUse blocks
            let tool_uses: Vec<_> = blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, name, input } => {
                        Some((id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect();

            if tool_uses.is_empty() {
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
            } else if tool_uses.len() == 1 {
                // Single tool use — check for accompanying reasoning (Reasoning block first,
                // then fallback to Text blocks for providers that don't use structured reasoning).
                let (reasoning, signature) = blocks
                    .iter()
                    .find_map(|b| {
                        if let ContentBlock::Reasoning { content, signature } = b {
                            Some((content.clone(), signature.clone()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        let text = blocks
                            .iter()
                            .filter_map(|b| match b {
                                ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        (text, None)
                    });

                let (_, name, input) = tool_uses.into_iter().next().unwrap();
                if reasoning.is_empty() {
                    Ok(Decision::ToolCall {
                        tool: name,
                        params: input,
                    })
                } else {
                    Ok(Decision::ThinkingToolCall {
                        reasoning,
                        signature,
                        tool: name,
                        params: input,
                    })
                }
            } else {
                // Multiple tool uses — return MultiToolCall
                use lattice_core::llm::ToolCallRequest;
                let calls = tool_uses
                    .into_iter()
                    .map(|(id, name, input)| ToolCallRequest {
                        id,
                        tool: name,
                        params: input,
                    })
                    .collect();
                Ok(Decision::MultiToolCall { calls })
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
        // Text alongside a ToolUse is treated as reasoning (ThinkingToolCall).
        match decision {
            Decision::ThinkingToolCall {
                tool, reasoning, ..
            } => {
                assert_eq!(tool, "bash");
                assert_eq!(reasoning, "thinking...");
            }
            _ => panic!("expected ThinkingToolCall, got {:?}", decision),
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

    /// Test for Issue #27: Multiple ToolUse blocks should return MultiToolCall
    #[test]
    fn test_mixed_with_multiple_tool_use() {
        let resp = LLMResponse::Mixed {
            blocks: vec![
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "ls"}),
                },
                ContentBlock::ToolUse {
                    id: "call_2".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "pwd"}),
                },
                ContentBlock::ToolUse {
                    id: "call_3".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "whoami"}),
                },
            ],
        };

        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::MultiToolCall { calls } => {
                assert_eq!(calls.len(), 3);
                assert_eq!(calls[0].id, "call_1");
                assert_eq!(calls[0].tool, "bash");
                assert_eq!(calls[0].params, serde_json::json!({"command": "ls"}));
                assert_eq!(calls[1].id, "call_2");
                assert_eq!(calls[2].id, "call_3");
            }
            _ => panic!("expected MultiToolCall, got {:?}", decision),
        }
    }

    /// Text alongside a single ToolUse is treated as reasoning and returns ThinkingToolCall.
    #[test]
    fn test_mixed_with_single_tool_use_returns_thinking_tool_call() {
        let resp = LLMResponse::Mixed {
            blocks: vec![
                ContentBlock::Text {
                    text: "Let me check".into(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ],
        };

        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::ThinkingToolCall {
                reasoning,
                tool,
                params,
                ..
            } => {
                assert_eq!(reasoning, "Let me check");
                assert_eq!(tool, "bash");
                assert_eq!(params, serde_json::json!({"command": "ls"}));
            }
            _ => panic!(
                "expected ThinkingToolCall for text+tool_use, got {:?}",
                decision
            ),
        }
    }

    /// Test mixed blocks with text and multiple tool uses
    #[test]
    fn test_mixed_with_text_and_multiple_tool_use() {
        let resp = LLMResponse::Mixed {
            blocks: vec![
                ContentBlock::Text {
                    text: "I'll check both files".into(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "cat file1.txt"}),
                },
                ContentBlock::ToolUse {
                    id: "call_2".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "cat file2.txt"}),
                },
            ],
        };

        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::MultiToolCall { calls } => {
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].tool, "bash");
                assert_eq!(calls[1].tool, "bash");
            }
            _ => panic!("expected MultiToolCall, got {:?}", decision),
        }
    }

    /// Test that text-only mixed blocks still return FinalAnswer
    #[test]
    fn test_mixed_text_only_no_tool_use() {
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
            Decision::FinalAnswer { answer } => {
                assert_eq!(answer, "part 1\npart 2");
            }
            _ => panic!("expected FinalAnswer, got {:?}", decision),
        }
    }

    /// Mixed with reasoning text + single ToolUse → ThinkingToolCall (DeepSeek thinking mode).
    /// Reasoning block (Anthropic thinking mode) + ToolUse → ThinkingToolCall with signature.
    #[test]
    fn test_mixed_with_reasoning_block_and_tool_use() {
        let resp = LLMResponse::Mixed {
            blocks: vec![
                ContentBlock::Reasoning {
                    content: "I should run df -h.".into(),
                    signature: Some("sig-abc123".into()),
                },
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "df -h"}),
                },
            ],
        };
        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::ThinkingToolCall {
                reasoning,
                signature,
                tool,
                params,
            } => {
                assert_eq!(reasoning, "I should run df -h.");
                assert_eq!(signature, Some("sig-abc123".into()));
                assert_eq!(tool, "bash");
                assert_eq!(params["command"], "df -h");
            }
            _ => panic!("expected ThinkingToolCall, got {:?}", decision),
        }
    }

    #[test]
    fn test_mixed_with_reasoning_text_and_tool_use_returns_thinking_tool_call() {
        let resp = LLMResponse::Mixed {
            blocks: vec![
                ContentBlock::Text {
                    text: "let me think about this step by step".into(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ],
        };

        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::ThinkingToolCall {
                reasoning,
                tool,
                params,
                ..
            } => {
                assert_eq!(reasoning, "let me think about this step by step");
                assert_eq!(tool, "bash");
                assert_eq!(params["command"], "ls");
            }
            _ => panic!("expected ThinkingToolCall, got {:?}", decision),
        }
    }

    /// Mixed with NO text + single ToolUse → plain ToolCall (no regression).
    #[test]
    fn test_mixed_with_no_text_and_tool_use_returns_tool_call() {
        let resp = LLMResponse::Mixed {
            blocks: vec![ContentBlock::ToolUse {
                id: "call_1".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "pwd"}),
            }],
        };

        let decision = response_to_decision(resp).unwrap();
        match decision {
            Decision::ToolCall { tool, params } => {
                assert_eq!(tool, "bash");
                assert_eq!(params["command"], "pwd");
            }
            _ => panic!("expected ToolCall, got {:?}", decision),
        }
    }
}
