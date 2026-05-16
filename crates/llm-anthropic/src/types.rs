//! Anthropic API request and response types.

use serde::{Deserialize, Serialize};

/// Anthropic Messages API request body.
#[derive(Debug, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AnthropicTool>,
}

/// A message in Anthropic format.
#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

/// Content can be a simple string or an array of content blocks.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicContent {
    /// Simple text string.
    Text(String),
    /// Array of content blocks.
    Blocks(Vec<AnthropicContentBlock>),
}

/// A content block in an Anthropic message.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    /// Text content.
    #[serde(rename = "text")]
    Text {
        /// The text.
        text: String,
    },
    /// Tool use request from the assistant.
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Unique ID for this tool use.
        id: String,
        /// Tool name.
        name: String,
        /// Tool input parameters.
        input: serde_json::Value,
    },
    /// Tool result provided by the user.
    #[serde(rename = "tool_result")]
    ToolResult {
        /// The tool_use ID this result corresponds to.
        tool_use_id: String,
        /// Result content.
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    /// Thinking block returned by reasoning-capable models (e.g. DeepSeek-V4-Flash).
    #[serde(rename = "thinking")]
    Thinking {
        /// The reasoning text.
        thinking: String,
        /// Opaque signature returned by the API; preserved for round-trip fidelity.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[allow(dead_code)]
        signature: Option<String>,
    },
}

/// An Anthropic tool definition.
#[derive(Debug, Serialize)]
pub struct AnthropicTool {
    /// Tool name.
    pub name: String,
    /// Description of the tool.
    pub description: String,
    /// JSON Schema for the tool input.
    pub input_schema: serde_json::Value,
}

/// Anthropic Messages API response.
#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    /// Response content blocks.
    pub content: Vec<AnthropicContentBlock>,
    /// Stop reason (e.g. "end_turn", "tool_use").
    #[serde(default)]
    #[allow(dead_code)]
    pub stop_reason: Option<String>,
    /// Usage information.
    #[serde(default)]
    #[allow(dead_code)]
    pub usage: Option<AnthropicUsage>,
}

/// Token usage information.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
}

/// Anthropic API error response.
#[derive(Debug, Deserialize)]
pub struct AnthropicError {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub error_type: Option<String>,
    pub error: Option<AnthropicErrorDetail>,
}

/// Detail within an Anthropic error response.
#[derive(Debug, Deserialize)]
pub struct AnthropicErrorDetail {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub error_type: Option<String>,
    pub message: String,
}
