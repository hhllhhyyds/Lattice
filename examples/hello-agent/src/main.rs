//! Hello Agent — end-to-end validation of the Lattice framework.
//!
//! Assembles `MemoryStore + LocalSandbox + BasicSandboxRouter + ControlLoop`
//! and drives them with a `MockLLMClient` that returns a fixed two-step decision
//! sequence: ToolCall (bash) → FinalAnswer.
//!
//! Run with:
//!   cargo run --example hello-agent

mod mock_llm;

use std::sync::Arc;

use anyhow::Result;
use lattice::core::{Actor, EventFilter, EventPayload};
use lattice::runtime::{BasicSandboxRouter, ControlLoop};
use lattice::sandbox_local::LocalSandbox;
use lattice::store_memory::MemoryStore;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    // ── 1. Assemble components ─────────────────────────────────────────────
    let store: Arc<dyn lattice::core::SessionStore> = Arc::new(MemoryStore::new());
    let sandbox = Arc::new(LocalSandbox::new());
    let llm = Arc::new(mock_llm::MockLLMClient::hello_agent_sequence());
    let router = Arc::new(BasicSandboxRouter::new(sandbox, store.clone()));

    let control_loop =
        ControlLoop::with_options(store.clone(), llm, router, vec![], String::new(), 50);

    // ── 2. Create session ───────────────────────────────────────────────────
    let session_id = control_loop.store().create_session().await?;
    info!(?session_id, "session created");

    // ── 3. Append user message ──────────────────────────────────────────────
    control_loop
        .store()
        .append_event(
            session_id,
            EventPayload::UserMessage {
                content: "Run 'echo Hello from Lattice!' and tell me the output.".to_string(),
            },
            Actor::System,
            None,
        )
        .await?;

    // ── 4. Run the agent ────────────────────────────────────────────────────
    let answer = control_loop.run(session_id).await?;
    println!("\n=== Agent Answer ===");
    println!("{}", answer);

    // ── 5. Print full event log ─────────────────────────────────────────────
    let events = store
        .get_events(session_id, &EventFilter::default())
        .await?;
    println!("\n=== Event Log ({} events) ===", events.len());
    for event in &events {
        println!("  [{:?}] {:?}", event.actor, event.payload);
    }

    Ok(())
}
