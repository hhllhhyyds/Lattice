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
