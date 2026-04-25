//! Lattice core: pure trait definitions and shared types.
//!
//! This crate contains zero runtime dependencies — only interfaces.

pub mod error;
mod event;
mod traits;

pub use error::{LLMError, RouterError, SandboxError, StoreError};
pub use event::*;
pub use traits::*;
