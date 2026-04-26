//! Error types for Lattice core.
//!
//! All errors use thiserror with structured variants for precise error handling.

use thiserror::Error;

use crate::event::SessionId;

/// Error from the session store.
#[derive(Debug, Clone, Error)]
#[error(transparent)]
pub enum StoreError {
    /// The requested session does not exist.
    #[error("session not found: {0}")]
    SessionNotFound(SessionId),
    /// Failed to serialize or deserialize data.
    #[error("serialization error: {0}")]
    SerializationError(String),
    /// Generic store error.
    #[error("store error: {0}")]
    Other(String),
}

/// Error from the LLM client.
#[derive(Debug, Clone, Error)]
#[error(transparent)]
pub enum LLMError {
    /// The LLM request failed (network, timeout, etc.).
    #[error("request failed: {0}")]
    RequestFailed(String),
    /// The LLM returned a response that could not be parsed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    /// Generic LLM error.
    #[error("LLM error: {0}")]
    Other(String),
}

/// Error from the sandbox.
#[derive(Debug, Clone, Error)]
#[error(transparent)]
pub enum SandboxError {
    /// Tool execution failed (non-zero exit code or panic).
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    /// Execution timed out.
    #[error("timeout after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },
    /// Sandbox is unavailable or not ready.
    #[error("sandbox unavailable: {0}")]
    Unavailable(String),
    /// Generic sandbox error.
    #[error("sandbox error: {0}")]
    Other(String),
}

/// Error from a tool executor.
#[derive(Debug, Clone, Error)]
#[error(transparent)]
pub enum ToolError {
    /// Tool not found in the registry.
    #[error("tool not found: {0}")]
    NotFound(String),
    /// Invalid parameters provided to the tool.
    #[error("invalid parameters: {0}")]
    InvalidParams(String),
    /// Tool execution failed.
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    /// Execution timed out.
    #[error("timeout after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },
    /// Generic tool error.
    #[error("tool error: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::StoreError;
    use super::{LLMError, SandboxError, ToolError};

    #[test]
    fn test_store_error_display() {
        let sid = uuid::Uuid::new_v4();
        let err = StoreError::SessionNotFound(sid);
        assert!(err.to_string().contains("session not found"));
        assert!(err.to_string().contains(&sid.to_string()));

        let err = StoreError::SerializationError("bad json".to_string());
        assert_eq!(err.to_string(), "serialization error: bad json");

        let err = StoreError::Other("oops".to_string());
        assert_eq!(err.to_string(), "store error: oops");
    }

    #[test]
    fn test_llm_error_display() {
        let err = LLMError::RequestFailed("timeout".to_string());
        assert_eq!(err.to_string(), "request failed: timeout");

        let err = LLMError::InvalidResponse("malformed".to_string());
        assert_eq!(err.to_string(), "invalid response: malformed");

        let err = LLMError::Other("oops".to_string());
        assert_eq!(err.to_string(), "LLM error: oops");
    }

    #[test]
    fn test_sandbox_error_display() {
        let err = SandboxError::ExecutionFailed("exit 1".to_string());
        assert_eq!(err.to_string(), "execution failed: exit 1");

        let err = SandboxError::Timeout { timeout_secs: 30 };
        assert!(err.to_string().contains("30"));
        assert!(err.to_string().contains("timeout"));

        let err = SandboxError::Unavailable("not ready".to_string());
        assert_eq!(err.to_string(), "sandbox unavailable: not ready");

        let err = SandboxError::Other("crash".to_string());
        assert_eq!(err.to_string(), "sandbox error: crash");
    }

    #[test]
    fn test_tool_error_display() {
        let err = ToolError::NotFound("bash".to_string());
        assert_eq!(err.to_string(), "tool not found: bash");

        let err = ToolError::InvalidParams("missing key".to_string());
        assert_eq!(err.to_string(), "invalid parameters: missing key");

        let err = ToolError::ExecutionFailed("segfault".to_string());
        assert_eq!(err.to_string(), "execution failed: segfault");

        let err = ToolError::Timeout { timeout_secs: 60 };
        assert!(err.to_string().contains("60"));
        assert!(err.to_string().contains("timeout"));

        let err = ToolError::Other("misc".to_string());
        assert_eq!(err.to_string(), "tool error: misc");
    }
}
