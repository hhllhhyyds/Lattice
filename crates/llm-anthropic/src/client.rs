//! Anthropic Claude LLMClient implementation.

use std::time::Duration;

use async_trait::async_trait;
use lattice_core::error::LLMError;
use lattice_core::llm::Decision;
use lattice_core::{Event, ToolDescription};
use lattice_llm_protocol::convert::{events_to_messages, tool_descriptions_to_specs};
use lattice_llm_protocol::message::{ContentBlock, Message, Role};
use lattice_llm_protocol::response::LLMResponse;
use tracing::{debug, instrument, warn};

use crate::types::*;

/// Default Anthropic API base URL.
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Default API version header.
const API_VERSION: &str = "2023-06-01";

/// Anthropic Claude LLM client.
///
/// Implements [`lattice_core::LLMClient`] using the Anthropic Messages API.
/// Supports custom base URLs for proxies and local deployments.
pub struct AnthropicClient {
    /// API key for authentication.
    api_key: String,
    /// Base URL (e.g. `https://api.anthropic.com`).
    base_url: String,
    /// Model name (e.g. `claude-sonnet-4-20250514`).
    model: String,
    /// Maximum tokens to generate.
    max_tokens: u32,
    /// HTTP client.
    http: reqwest::Client,
}

impl AnthropicClient {
    /// Create a new Anthropic client with default settings.
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

    /// Set a custom base URL (for proxies or local deployments).
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

    /// Convert protocol messages to Anthropic format.
    fn to_anthropic_messages(&self, messages: &[Message]) -> Vec<AnthropicMessage> {
        messages
            .iter()
            .filter_map(|msg| {
                let role = match msg.role {
                    Role::User | Role::Tool => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                    Role::System => {
                        warn!(
                            "system-role message found in Anthropic conversation history; skipping"
                        );
                        return None;
                    }
                };

                let content = self.to_anthropic_content(&msg.content, msg.role);
                Some(AnthropicMessage { role, content })
            })
            .collect()
    }

    /// Convert content blocks to Anthropic format.
    fn to_anthropic_content(&self, blocks: &[ContentBlock], role: Role) -> AnthropicContent {
        if blocks.len() == 1 {
            if let ContentBlock::Text { text } = &blocks[0] {
                if role != Role::Tool {
                    return AnthropicContent::Text(text.clone());
                }
            }
        }

        let anthropic_blocks = blocks
            .iter()
            .map(|b| match b {
                ContentBlock::Text { text } => AnthropicContentBlock::Text { text: text.clone() },
                ContentBlock::ToolUse { id, name, input } => AnthropicContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                },
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => AnthropicContentBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error: if *is_error { Some(true) } else { None },
                },
            })
            .collect();

        AnthropicContent::Blocks(anthropic_blocks)
    }

    /// Convert tool descriptions to Anthropic tool format.
    fn to_anthropic_tools(&self, tools: &[ToolDescription]) -> Vec<AnthropicTool> {
        let specs = tool_descriptions_to_specs(tools);
        specs
            .into_iter()
            .map(|s| AnthropicTool {
                name: s.name,
                description: s.description,
                input_schema: s.input_schema,
            })
            .collect()
    }

    /// Parse an Anthropic response into a provider-agnostic LLMResponse.
    fn parse_response(&self, response: AnthropicResponse) -> LLMResponse {
        let blocks = &response.content;

        if blocks.len() == 1 {
            match &blocks[0] {
                AnthropicContentBlock::Text { text } => {
                    return LLMResponse::Text { text: text.clone() };
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    return LLMResponse::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    };
                }
                _ => {}
            }
        }

        // Multiple blocks → Mixed
        let content_blocks: Vec<ContentBlock> = blocks
            .iter()
            .filter_map(|b| match b {
                AnthropicContentBlock::Text { text } => {
                    Some(ContentBlock::Text { text: text.clone() })
                }
                AnthropicContentBlock::ToolUse { id, name, input } => Some(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }),
                _ => None,
            })
            .collect();

        LLMResponse::Mixed {
            blocks: content_blocks,
        }
    }
}

#[async_trait]
impl lattice_core::LLMClient for AnthropicClient {
    #[instrument(skip(self, history, available_tools))]
    async fn decide(
        &self,
        history: &[Event],
        available_tools: &[ToolDescription],
        system_prompt: &str,
    ) -> Result<Decision, LLMError> {
        // Convert events to protocol messages, then to Anthropic format.
        let protocol_messages = events_to_messages(history);
        let messages = self.to_anthropic_messages(&protocol_messages);
        let tools = self.to_anthropic_tools(available_tools);

        let request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: if system_prompt.is_empty() {
                None
            } else {
                Some(system_prompt.to_string())
            },
            messages,
            tools,
        };

        debug!("sending request to Anthropic API");

        let url = format!("{}/v1/messages", self.base_url);
        let http_response = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
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
            // Try to parse error details.
            if let Ok(err) = serde_json::from_str::<AnthropicError>(&body) {
                let msg = err
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| format!("HTTP {status}"));
                return Err(LLMError::RequestFailed(msg));
            }
            return Err(LLMError::RequestFailed(format!("HTTP {status}: {body}")));
        }

        let response: AnthropicResponse =
            serde_json::from_str(&body).map_err(|e| LLMError::InvalidResponse(e.to_string()))?;

        let llm_response = self.parse_response(response);
        lattice_llm_protocol::response_to_decision(llm_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_response() {
        let client = AnthropicClient::new("test-key", "claude-sonnet-4-20250514");
        let response = AnthropicResponse {
            content: vec![AnthropicContentBlock::Text {
                text: "Hello!".into(),
            }],
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        let result = client.parse_response(response);
        match result {
            LLMResponse::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("expected Text response"),
        }
    }

    #[test]
    fn test_parse_tool_use_response() {
        let client = AnthropicClient::new("test-key", "claude-sonnet-4-20250514");
        let response = AnthropicResponse {
            content: vec![AnthropicContentBlock::ToolUse {
                id: "toolu_123".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "ls"}),
            }],
            stop_reason: Some("tool_use".into()),
            usage: None,
        };
        let result = client.parse_response(response);
        match result {
            LLMResponse::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_123");
                assert_eq!(name, "bash");
                assert_eq!(input, serde_json::json!({"command": "ls"}));
            }
            _ => panic!("expected ToolUse response"),
        }
    }

    #[test]
    fn test_parse_mixed_response() {
        let client = AnthropicClient::new("test-key", "claude-sonnet-4-20250514");
        let response = AnthropicResponse {
            content: vec![
                AnthropicContentBlock::Text {
                    text: "Let me check.".into(),
                },
                AnthropicContentBlock::ToolUse {
                    id: "toolu_456".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "pwd"}),
                },
            ],
            stop_reason: Some("tool_use".into()),
            usage: None,
        };
        let result = client.parse_response(response);
        match result {
            LLMResponse::Mixed { blocks } => {
                assert_eq!(blocks.len(), 2);
            }
            _ => panic!("expected Mixed response"),
        }
    }

    #[test]
    fn test_to_anthropic_tools() {
        let client = AnthropicClient::new("test-key", "claude-sonnet-4-20250514");
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
        let anthropic_tools = client.to_anthropic_tools(&tools);
        assert_eq!(anthropic_tools.len(), 1);
        assert_eq!(anthropic_tools[0].name, "bash");
    }

    #[test]
    fn test_to_anthropic_messages_user() {
        let client = AnthropicClient::new("key", "claude");
        let messages = vec![Message::text(Role::User, "hello")];
        let result = client.to_anthropic_messages(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
        match &result[0].content {
            AnthropicContent::Text(t) => assert_eq!(t, "hello"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn test_to_anthropic_messages_assistant() {
        let client = AnthropicClient::new("key", "claude");
        let messages = vec![Message::text(Role::Assistant, "I think...")];
        let result = client.to_anthropic_messages(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "assistant");
    }

    #[test]
    fn test_to_anthropic_messages_system() {
        let client = AnthropicClient::new("key", "claude");
        // System prompt is handled by the top-level request field, not history.
        let messages = vec![Message::text(Role::System, "instructions")];
        let result = client.to_anthropic_messages(&messages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_to_anthropic_messages_skips_system_without_dropping_neighbors() {
        let client = AnthropicClient::new("key", "claude");
        let messages = vec![
            Message::text(Role::User, "hello"),
            Message::text(Role::System, "must not become user content"),
            Message::text(Role::Assistant, "hi"),
        ];

        let result = client.to_anthropic_messages(&messages);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[1].role, "assistant");
        assert!(result.iter().all(|msg| match &msg.content {
            AnthropicContent::Text(text) => !text.contains("must not become user content"),
            AnthropicContent::Blocks(_) => true,
        }));
    }

    #[test]
    fn test_to_anthropic_content_single_text_non_tool_role() {
        let client = AnthropicClient::new("key", "claude");
        let blocks = vec![ContentBlock::Text {
            text: "hello".into(),
        }];
        let result = client.to_anthropic_content(&blocks, Role::User);
        match result {
            AnthropicContent::Text(t) => assert_eq!(t, "hello"),
            _ => panic!("expected Text, got Blocks"),
        }
    }

    #[test]
    fn test_to_anthropic_content_single_text_tool_role() {
        let client = AnthropicClient::new("key", "claude");
        // Tool role with single text → Blocks (not simple Text)
        let blocks = vec![ContentBlock::Text {
            text: "hello".into(),
        }];
        let result = client.to_anthropic_content(&blocks, Role::Tool);
        match &result {
            AnthropicContent::Blocks(b) => assert_eq!(b.len(), 1),
            _ => panic!("expected Blocks"),
        }
    }

    #[test]
    fn test_to_anthropic_content_tool_result() {
        let client = AnthropicClient::new("key", "claude");
        let blocks = vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_1".into(),
            content: "result output".into(),
            is_error: false,
        }];
        let result = client.to_anthropic_content(&blocks, Role::User);
        match &result {
            AnthropicContent::Blocks(b) => {
                assert_eq!(b.len(), 1);
                match &b[0] {
                    AnthropicContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        assert_eq!(tool_use_id, "toolu_1");
                        assert_eq!(content, "result output");
                        assert!(is_error.is_none());
                    }
                    _ => panic!("expected ToolResult block"),
                }
            }
            _ => panic!("expected Blocks"),
        }
    }

    #[test]
    fn test_to_anthropic_content_tool_result_with_error() {
        let client = AnthropicClient::new("key", "claude");
        let blocks = vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_2".into(),
            content: "error msg".into(),
            is_error: true,
        }];
        let result = client.to_anthropic_content(&blocks, Role::User);
        match &result {
            AnthropicContent::Blocks(b) => match &b[0] {
                AnthropicContentBlock::ToolResult { is_error, .. } => {
                    assert_eq!(is_error, &Some(true));
                }
                _ => panic!("expected ToolResult"),
            },
            _ => panic!("expected Blocks"),
        }
    }

    #[test]
    fn test_parse_text_response_single_block() {
        let client = AnthropicClient::new("key", "claude");
        let response = AnthropicResponse {
            content: vec![AnthropicContentBlock::Text { text: "hi".into() }],
            stop_reason: None,
            usage: None,
        };
        let result = client.parse_response(response);
        match result {
            LLMResponse::Text { text } => assert_eq!(text, "hi"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn test_request_serialization() {
        let request = AnthropicRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            system: Some("You are helpful.".into()),
            messages: vec![AnthropicMessage {
                role: "user".into(),
                content: AnthropicContent::Text("Hello".into()),
            }],
            tools: vec![],
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "claude-sonnet-4-20250514");
        assert_eq!(json["max_tokens"], 1024);
        assert_eq!(json["system"], "You are helpful.");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
        // tools should be absent (skip_serializing_if empty)
        assert!(json.get("tools").is_none());
    }
}
