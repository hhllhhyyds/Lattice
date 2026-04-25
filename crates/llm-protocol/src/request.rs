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
