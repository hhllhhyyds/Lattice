//! Lattice LLM backend for Anthropic Claude.
//!
//! Implements [`lattice_core::LLMClient`] using the Anthropic Messages API.

mod client;
mod types;

pub use client::AnthropicClient;
