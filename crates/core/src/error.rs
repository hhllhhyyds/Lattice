//! Error types for Lattice core.

use thiserror::Error;

/// Error from the session store.
#[derive(Debug, Error)]
#[error("store error: {0}")]
pub struct StoreError(pub String);

/// Error from the LLM client.
#[derive(Debug, Error)]
#[error("LLM error: {0}")]
pub struct LLMError(pub String);

/// Error from the sandbox.
#[derive(Debug, Error)]
#[error("sandbox error: {0}")]
pub struct SandboxError(pub String);

/// Error from the sandbox router.
#[derive(Debug, Error)]
#[error("router error: {0}")]
pub struct RouterError(pub String);
