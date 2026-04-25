//! Lattice LLM protocol — provider-agnostic message types and conversion logic.
//!
//! This crate bridges the gap between Lattice's event-sourced architecture
//! and the various LLM provider APIs. It provides:
//!
//! - A universal message format ([`Message`], [`ContentBlock`], [`Role`])
//! - Conversion from Lattice [`lattice_core::Event`]s to LLM messages ([`convert`])
//! - Parsing of LLM responses into [`lattice_core::Decision`]s ([`parse`])
//! - Provider-agnostic request/response types ([`LLMRequest`], [`LLMResponse`])

pub mod convert;
pub mod message;
pub mod parse;
pub mod request;
pub mod response;

pub use convert::events_to_messages;
pub use message::{ContentBlock, Message, Role};
pub use parse::response_to_decision;
pub use request::{LLMRequest, ToolSpec};
pub use response::LLMResponse;
