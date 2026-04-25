//! Lattice runtime: ControlLoop implementation.
//!
//! This crate provides the agent control loop — the central orchestrator that
//! loads event history, calls the LLM for decisions, routes tool calls, and
//! records results. It is stateless and recovers all state from the SessionStore.
//!
//! # Core types
//!
//! - [`ControlLoop`] — the agent brain, drives the decision cycle
//! - [`BasicSandboxRouter`] — default router that forwards tool calls to a sandbox
//!
//! # Example
//!
//! ```ignore
//! let store = Arc::new(MemoryStore::new());
//! let sandbox = Arc::new(LocalSandbox::new());
//! let llm = Arc::new(my_llm_client);
//! let router = Arc::new(BasicSandboxRouter::new(sandbox, store.clone()));
//! let control_loop = ControlLoop::new(store, llm, router);
//! control_loop.run(session_id).await?;
//! ```

mod control_loop;
mod router;

pub use control_loop::ControlLoop;
pub use router::BasicSandboxRouter;
