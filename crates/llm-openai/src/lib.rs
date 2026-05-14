//! Lattice LLM backend for OpenAI-compatible APIs.
//!
//! Implements [`lattice_core::LLMClient`] using the OpenAI Chat Completions API format.
//! Compatible with OpenAI, local deployments (vLLM, Ollama), and third-party proxies.

mod client;
mod codex_cli;
mod types;

pub use client::OpenAIClient;
pub use codex_cli::CodexCliClient;
