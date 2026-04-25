//! Lattice LLM backend for OpenAI-compatible APIs.
//!
//! Implements [`LLMClient`] using the OpenAI Chat Completions API format.
//! Compatible with OpenAI, local deployments (vLLM, Ollama), and third-party proxies.

mod client;
mod types;

pub use client::OpenAIClient;
