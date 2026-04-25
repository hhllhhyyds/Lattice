//! OpenAI-compatible LLMClient implementation.

use std::time::Duration;

use async_trait::async_trait;
use lattice_core::error::LLMError;
use lattice_core::llm::Decision;
use lattice_core::{Event, ToolDescription};
use lattice_llm_protocol::convert::{events_to_messages, tool_descriptions_to_specs};
use lattice_llm_protocol::message::{ContentBlock, Message, Role};
use lattice_llm_protocol::response::LLMResponse;
use tracing::{debug, instrument};

use crate::types::*;

/// Default OpenAI API base URL.
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// OpenAI-compatible LLM client.
///
/// Implements [`lattice_core::LLMClient`] using the Chat Completions API.
/// Works with OpenAI, vLLM, Ollama, and any OpenAI-compatible endpoint.
pub struct OpenAIClient {
    /// API key for authentication.
    api_key: String,
    /// Base URL (e.g. `https://api.openai.com/v1`).
    base_url: String,
    /// Model name (e.g. `gpt-4o`).
    model: String,
    /// Maximum tokens to generate.
    max_tokens: u32,
    /// HTTP client.
    http: reqwest::Client,
}

impl OpenAIClient {
    /// Create a new OpenAI-compatible client with default settings.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");

        Self {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: model.into(),
            max_tokens: 4096,
            http,
        }
    }

    /// Set a custom base URL (for local deployments or proxies).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Set max tokens.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Convert protocol messages to OpenAI format.
    fn to_openai_messages(&self, messages: &[Message], system_prompt: &str) -> Vec<OpenAIMessage> {
        let mut result = Vec::new();

        // Add system message first.
        if !system_prompt.is_empty() {
            result.push(OpenAIMessage {
                role: "system".into(),
                content: Some(system_prompt.to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        }

        for msg in messages {
            match msg.role {
                Role::User => {
                    let text = extract_text(&msg.content);
                    result.push(OpenAIMessage {
                        role: "user".into(),
                        content: Some(text),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
                Role::Assistant => {
                    // Check if this message contains tool use blocks.
                    let tool_uses: Vec<_> = msg
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::ToolUse { id, name, input } => Some(OpenAIToolCall {
                                id: id.clone(),
                                call_type: "function".into(),
                                function: OpenAIFunctionCall {
                                    name: name.clone(),
                                    arguments: serde_json::to_string(input).unwrap_or_default(),
                                },
                            }),
                            _ => None,
                        })
                        .collect();

                    let text = extract_text(&msg.content);

                    if tool_uses.is_empty() {
                        result.push(OpenAIMessage {
                            role: "assistant".into(),
                            content: if text.is_empty() { None } else { Some(text) },
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                        });
                    } else {
                        result.push(OpenAIMessage {
                            role: "assistant".into(),
                            content: if text.is_empty() { None } else { Some(text) },
                            tool_calls: Some(tool_uses),
                            tool_call_id: None,
                            name: None,
                        });
                    }
                }
                Role::Tool => {
                    // Tool results in OpenAI format.
                    for block in &msg.content {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } = block
                        {
                            result.push(OpenAIMessage {
                                role: "tool".into(),
                                content: Some(content.clone()),
                                tool_calls: None,
                                tool_call_id: Some(tool_use_id.clone()),
                                name: None,
                            });
                        }
                    }
                }
                Role::System => {
                    // Already handled above.
                }
            }
        }

        result
    }

    /// Convert tool descriptions to OpenAI tool format.
    fn to_openai_tools(&self, tools: &[ToolDescription]) -> Vec<OpenAITool> {
        let specs = tool_descriptions_to_specs(tools);
        specs
            .into_iter()
            .map(|s| OpenAITool {
                tool_type: "function".into(),
                function: OpenAIFunction {
                    name: s.name,
                    description: s.description,
                    parameters: s.input_schema,
                },
            })
            .collect()
    }

    /// Parse an OpenAI response into a provider-agnostic LLMResponse.
    fn parse_response(&self, response: OpenAIResponse) -> Result<LLMResponse, LLMError> {
        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LLMError::InvalidResponse("response contained no choices".into()))?;

        let msg = choice.message;

        // Check for tool calls first.
        if let Some(tool_calls) = msg.tool_calls {
            if let Some(tc) = tool_calls.into_iter().next() {
                let input: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or_else(|_| serde_json::Value::String(tc.function.arguments.clone()));
                return Ok(LLMResponse::ToolUse {
                    id: tc.id,
                    name: tc.function.name,
                    input,
                });
            }
        }

        // Fall back to text content.
        if let Some(text) = msg.content {
            Ok(LLMResponse::Text { text })
        } else {
            Err(LLMError::InvalidResponse(
                "response contained no content or tool_calls".into(),
            ))
        }
    }
}

/// Extract concatenated text from content blocks.
fn extract_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[async_trait]
impl lattice_core::LLMClient for OpenAIClient {
    #[instrument(skip(self, history, available_tools))]
    async fn decide(
        &self,
        history: &[Event],
        available_tools: &[ToolDescription],
        system_prompt: &str,
    ) -> Result<Decision, LLMError> {
        // Convert events to protocol messages, then to OpenAI format.
        let protocol_messages = events_to_messages(history);
        let messages = self.to_openai_messages(&protocol_messages, system_prompt);
        let tools = self.to_openai_tools(available_tools);

        let request = OpenAIRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(self.max_tokens),
            tools,
        };

        debug!("sending request to OpenAI-compatible API");

        let url = format!("{}/chat/completions", self.base_url);
        let http_response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| LLMError::RequestFailed(e.to_string()))?;

        let status = http_response.status();
        let body = http_response
            .text()
            .await
            .map_err(|e| LLMError::RequestFailed(e.to_string()))?;

        if !status.is_success() {
            if let Ok(err) = serde_json::from_str::<OpenAIError>(&body) {
                return Err(LLMError::RequestFailed(err.error.message));
            }
            return Err(LLMError::RequestFailed(format!("HTTP {status}: {body}")));
        }

        let response: OpenAIResponse =
            serde_json::from_str(&body).map_err(|e| LLMError::InvalidResponse(e.to_string()))?;

        let llm_response = self.parse_response(response)?;
        lattice_llm_protocol::response_to_decision(llm_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_response() {
        let client = OpenAIClient::new("test-key", "gpt-4o");
        let response = OpenAIResponse {
            choices: vec![OpenAIChoice {
                message: OpenAIResponseMessage {
                    role: "assistant".into(),
                    content: Some("Hello!".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        };
        let result = client.parse_response(response).unwrap();
        match result {
            LLMResponse::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("expected Text response"),
        }
    }

    #[test]
    fn test_parse_tool_call_response() {
        let client = OpenAIClient::new("test-key", "gpt-4o");
        let response = OpenAIResponse {
            choices: vec![OpenAIChoice {
                message: OpenAIResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "call_123".into(),
                        call_type: "function".into(),
                        function: OpenAIFunctionCall {
                            name: "bash".into(),
                            arguments: r#"{"command":"ls"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let result = client.parse_response(response).unwrap();
        match result {
            LLMResponse::ToolUse { id, name, input } => {
                assert_eq!(id, "call_123");
                assert_eq!(name, "bash");
                assert_eq!(input, serde_json::json!({"command": "ls"}));
            }
            _ => panic!("expected ToolUse response"),
        }
    }

    #[test]
    fn test_parse_empty_choices() {
        let client = OpenAIClient::new("test-key", "gpt-4o");
        let response = OpenAIResponse {
            choices: vec![],
            usage: None,
        };
        let result = client.parse_response(response);
        assert!(result.is_err());
    }

    #[test]
    fn test_to_openai_tools() {
        let client = OpenAIClient::new("test-key", "gpt-4o");
        let tools = vec![ToolDescription {
            name: "bash".into(),
            description: "Execute bash commands".into(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"}
                },
                "required": ["command"]
            }),
        }];
        let openai_tools = client.to_openai_tools(&tools);
        assert_eq!(openai_tools.len(), 1);
        assert_eq!(openai_tools[0].tool_type, "function");
        assert_eq!(openai_tools[0].function.name, "bash");
    }

    #[test]
    fn test_request_serialization() {
        let request = OpenAIRequest {
            model: "gpt-4o".into(),
            messages: vec![OpenAIMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }],
            max_tokens: Some(1024),
            tools: vec![],
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "gpt-4o");
        assert_eq!(json["max_tokens"], 1024);
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
        // tools should be absent (skip_serializing_if empty)
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn test_to_openai_messages_with_system() {
        let client = OpenAIClient::new("test-key", "gpt-4o");
        let messages = vec![Message::text(Role::User, "hi")];
        let result = client.to_openai_messages(&messages, "Be helpful.");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[0].content.as_deref(), Some("Be helpful."));
        assert_eq!(result[1].role, "user");
        assert_eq!(result[1].content.as_deref(), Some("hi"));
    }
}
