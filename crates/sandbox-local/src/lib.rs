//! Lattice local sandbox: subprocess-based tool execution.
//!
//! Provides [`LocalSandbox`], a sandbox implementation that executes commands
//! as local subprocesses via `tokio::process::Command`.
//!
//! # Security
//!
//! **This provides no isolation.** Commands run with the same OS user privileges
//! as the Lattice process. Do not use in untrusted environments.

mod local_sandbox;

pub use local_sandbox::LocalSandbox;
