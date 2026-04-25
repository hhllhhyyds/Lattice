//! Hello Agent: end-to-end validation of the Lattice framework.

mod mock_llm;

use std::sync::Arc;

use anyhow::Result;
use lattice_core::Actor;
use lattice_runtime::{BasicSandboxRouter, ControlLoop};
use lattice_sandbox_local::LocalSandbox;
use lattice_store_memory::MemoryStore;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let store: Arc<dyn lattice_core::SessionStore> = Arc::new(MemoryStore::new());
    let sandbox = Arc::new(LocalSandbox::new());
    let llm = Arc::new(mock_llm::MockLLMClient::new());
    let router = Arc::new(BasicSandboxRouter::new(sandbox, store.clone()));
    let control_loop = ControlLoop::new(store.clone(), llm, router);

    let session_id = control_loop.store().create_session().await?;
    info!(?session_id, "session created");

    control_loop
        .store()
        .append_event(
            session_id,
            lattice_core::EventPayload::UserMessage {
                content: "Hello, agent!".to_string(),
            },
            Actor::System,
            None,
        )
        .await?;

    control_loop.run(session_id).await?;

    info!("agent finished");
    Ok(())
}
