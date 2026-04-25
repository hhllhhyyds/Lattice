//! Lattice LLM backend for Anthropic Claude.
//!
//! Implements [`LLMClient`] using the Anthropic Messages API.

mod client;
mod types;

pub use client::AnthropicClient;
