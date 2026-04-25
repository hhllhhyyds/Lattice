//! Provider-agnostic LLM response types.

use serde::{Deserialize, Serialize};

use crate::message::ContentBlock;

/// A provider-agnostic LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LLMResponse {
    /// Pure text response.
    Text {
        /// The response text.
        text: String,
    },
    /// Tool use response.
    ToolUse {
        /// Unique identifier for this tool use.
        id: String,
        /// Name of the tool to invoke.
        name: String,
        /// Tool input parameters.
        input: serde_json::Value,
    },
    /// Mixed response containing multiple content blocks.
    Mixed {
        /// Content blocks from the response.
        blocks: Vec<ContentBlock>,
    },
    /// Error response from the provider.
    Error {
        /// Error message.
        message: String,
    },
}
