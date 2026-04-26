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
//! ```text
//! // Create components (using your implementations):
//! let store: Arc<dyn SessionStore> = ...;
//! let llm: Arc<dyn LLMClient> = ...;
//! let sandbox: Arc<dyn Sandbox> = ...;
//!
//! // Build tool set and control loop
//! let tools = Arc::new(ToolSet::with_defaults(sandbox));
//! let control_loop = ControlLoop::new(store, llm, tools);
//!
//! // Run the agent
//! let answer = control_loop.run(session_id).await?;
//! ```

mod control_loop;

pub use control_loop::ControlLoop;
