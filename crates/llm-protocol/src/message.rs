//! Universal message types for LLM communication.

use serde::{Deserialize, Serialize};

/// Role in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System instructions.
    System,
    /// User input.
    User,
    /// Assistant (LLM) output.
    Assistant,
    /// Tool execution result.
    Tool,
}

/// A content block within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text {
        /// The text content.
        text: String,
    },
    /// Internal reasoning from a thinking-capable model (e.g. DeepSeek, Claude extended thinking).
    ///
    /// Must be passed back to the API in subsequent requests for models that require it.
    /// The `signature` is an opaque token required by some providers (e.g. DeepSeek Anthropic-compat)
    /// to verify round-trip integrity.
    Reasoning {
        /// The reasoning / thinking content.
        content: String,
        /// Opaque signature required by some providers for round-trip verification.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// A tool invocation by the assistant.
    ToolUse {
        /// Unique identifier for this tool use (used to correlate results).
        id: String,
        /// Name of the tool to invoke.
        name: String,
        /// Tool input parameters as JSON.
        input: serde_json::Value,
    },
    /// Result of a tool invocation.
    ToolResult {
        /// The tool_use id this result corresponds to.
        tool_use_id: String,
        /// The result content (text).
        content: String,
        /// Whether the tool call resulted in an error.
        is_error: bool,
    },
}

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender.
    pub role: Role,
    /// Content blocks that make up this message.
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Create a simple text message.
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }
}
