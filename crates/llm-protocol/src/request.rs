//! Provider-agnostic LLM request types.

use serde::{Deserialize, Serialize};

use crate::message::Message;

/// A tool specification for the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Tool name (must be unique within a request).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    pub input_schema: serde_json::Value,
}

/// A provider-agnostic LLM request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    /// System prompt / instructions.
    pub system: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Available tools.
    pub tools: Vec<ToolSpec>,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
}

impl LLMRequest {
    /// Create a new request with the given system prompt and messages.
    pub fn new(system: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            system: system.into(),
            messages,
            tools: Vec::new(),
            max_tokens: 4096,
        }
    }

    /// Set available tools.
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = tools;
        self
    }

    /// Set max tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_request_new() {
        let req = LLMRequest::new("system prompt", vec![]);
        assert_eq!(req.system, "system prompt");
        assert!(req.messages.is_empty());
        assert!(req.tools.is_empty());
        assert_eq!(req.max_tokens, 4096);
    }

    #[test]
    fn test_llm_request_builder_pattern() {
        let tools = vec![ToolSpec {
            name: "bash".into(),
            description: "Run bash".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let req = LLMRequest::new("system", vec![])
            .with_tools(tools.clone())
            .with_max_tokens(2048);
        assert_eq!(req.system, "system");
        assert_eq!(req.tools.len(), 1);
        assert_eq!(req.tools[0].name, "bash");
        assert_eq!(req.max_tokens, 2048);
    }

    #[test]
    fn test_tool_spec_serde() {
        let spec = ToolSpec {
            name: "test".into(),
            description: "A test tool".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"cmd": {"type": "string"}},
                "required": ["cmd"]
            }),
        };
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["name"], "test");
        assert_eq!(json["description"], "A test tool");
        assert_eq!(json["input_schema"]["type"], "object");

        let roundtrip: ToolSpec = serde_json::from_value(json).unwrap();
        assert_eq!(roundtrip.name, "test");
        assert_eq!(
            roundtrip.input_schema["properties"]["cmd"]["type"],
            "string"
        );
    }

    #[test]
    fn test_llm_request_serde() {
        let req = LLMRequest::new("system", vec![]).with_max_tokens(1024);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["system"], "system");
        assert_eq!(json["max_tokens"], 1024);
        assert!(json["messages"].is_array());
    }
}
