//! Lattice runtime: ControlLoop implementation.
//!
//! This crate provides the agent control loop — the central orchestrator that
//! loads event history, calls the LLM for decisions, routes tool calls, and
//! records results. It is stateless and recovers all state from the SessionStore.
//!
//! # Core types
//!
//! - [`ControlLoop`] — the agent brain, drives the decision cycle
//!
//! # Example
//!
//! ```ignore
//! use lattice_tools::ToolSet;
//!
//! let store = Arc::new(MemoryStore::new());
//! let sandbox = Arc::new(LocalSandbox::new());
//! let llm = Arc::new(my_llm_client);
//! let tools = Arc::new(ToolSet::with_defaults(sandbox));
//! let control_loop = ControlLoop::new(store, llm, tools);
//! control_loop.run(session_id).await?;
//! ```

mod control_loop;

pub use control_loop::ControlLoop;
