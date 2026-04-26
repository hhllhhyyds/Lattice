//! # Lattice
//!
//! A Rust meta-framework for building AI agents, inspired by Anthropic's managed
//! agents architecture.
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `runtime` | ✅ | ControlLoop (uses ToolSet) |
//! | `store-memory` | ✅ | In-memory SessionStore implementation |
//! | `sandbox-local` | ✅ | Local process Sandbox implementation |
//! | `tools` | ✅ | ToolSet registry + standard tools |
//! | `llm-protocol` | ❌ | Common LLM protocol layer |
//! | `llm-anthropic` | ❌ | Anthropic Claude LLM backend |
//! | `llm-openai` | ❌ | OpenAI-compatible LLM backend |
//! | `llm-all` | ❌ | All LLM backends |
//! | `full` | ❌ | Everything |

/// Core traits and types. Always available.
pub use lattice_core as core;

/// ControlLoop implementation (uses ToolSet).
#[cfg(feature = "runtime")]
pub use lattice_runtime as runtime;

/// In-memory SessionStore.
#[cfg(feature = "store-memory")]
pub use lattice_store_memory as store_memory;

/// Local process Sandbox.
#[cfg(feature = "sandbox-local")]
pub use lattice_sandbox_local as sandbox_local;

/// ToolSet registry and standard tool implementations.
#[cfg(feature = "tools")]
pub use lattice_tools as tools;

/// Common LLM protocol layer.
#[cfg(feature = "llm-protocol")]
pub use lattice_llm_protocol as llm_protocol;

/// Anthropic Claude LLM backend.
#[cfg(feature = "llm-anthropic")]
pub use lattice_llm_anthropic as llm_anthropic;

/// OpenAI-compatible LLM backend.
#[cfg(feature = "llm-openai")]
pub use lattice_llm_openai as llm_openai;
