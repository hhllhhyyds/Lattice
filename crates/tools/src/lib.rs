//! Lattice tools: ToolSet registry and standard tool implementations.

pub mod set;

#[cfg(feature = "bash")]
pub mod bash;

pub use set::ToolSet;

// Re-export ToolExecutor for convenience.
pub use lattice_core::ToolExecutor;
