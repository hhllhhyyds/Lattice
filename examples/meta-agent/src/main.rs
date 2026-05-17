//! Meta agent example — demonstrates skill delegation.
//!
//! This example shows how a meta agent can delegate complex subtasks to
//! specialized skill agents. The skill agent runs in its own sub-session
//! with its own ControlLoop and can use a subset of the parent's tools.
//!
//! Run with:
//!   cargo run --example meta-agent
//!
//! With real LLM (requires LATTICE_* env vars — see examples/real-agent for details):
//!   LATTICE_LLM_PROVIDER=anthropic LATTICE_API_KEY=sk-... LATTICE_MODEL=claude-sonnet-4-20250514 \
//!   LATTICE_ANTHROPIC_API_BASE=https://api.anthropic.com cargo run --example meta-agent

mod mock_llm;

use std::sync::Arc;

use anyhow::Result;
use lattice::core::{Actor, EventFilter, EventPayload, SessionStore, ToolExecutor};
use lattice::runtime::ControlLoop;
use lattice::sandbox_local::LocalSandbox;
use lattice::skill::SkillLoader;
use lattice::store_memory::MemoryStore;
use lattice::tools::ToolSet;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn build_llm_client() -> Option<Arc<dyn lattice::core::LLMClient>> {
    let provider = std::env::var("LATTICE_LLM_PROVIDER").ok()?;
    let api_key = std::env::var("LATTICE_API_KEY").ok()?;
    let client: Arc<dyn lattice::core::LLMClient> = match provider.as_str() {
        "anthropic" => {
            let base = std::env::var("LATTICE_ANTHROPIC_API_BASE")
                .or_else(|_| std::env::var("LATTICE_API_BASE"))
                .ok()?;
            let model = std::env::var("LATTICE_MODEL").ok()?;
            info!(model = %model, api_base = %base, "using real Anthropic LLM");
            Arc::new(
                lattice::llm_anthropic::AnthropicClient::new(&api_key, &model).with_base_url(base),
            )
        }
        "openai" | "openai-compatible" => {
            let base = std::env::var("LATTICE_OPENAI_API_BASE")
                .or_else(|_| std::env::var("LATTICE_API_BASE"))
                .ok()?;
            let model = std::env::var("LATTICE_OPENAI_MODEL")
                .or_else(|_| std::env::var("LATTICE_MODEL"))
                .ok()?;
            info!(model = %model, api_base = %base, "using real OpenAI-compatible LLM");
            Arc::new(lattice::llm_openai::OpenAIClient::new(&api_key, &model).with_base_url(base))
        }
        other => {
            info!("unsupported LATTICE_LLM_PROVIDER '{other}', falling back to mock");
            return None;
        }
    };
    Some(client)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    // ── 1. Assemble components ─────────────────────────────────────────────
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::new());
    let sandbox = Arc::new(LocalSandbox::new());

    // Use real LLM if LATTICE_* env vars are set, otherwise fall back to mock.
    let llm: Arc<dyn lattice::core::LLMClient> = match build_llm_client() {
        Some(client) => client,
        None => {
            info!("Using mock LLM (set LATTICE_LLM_PROVIDER / LATTICE_API_KEY / LATTICE_MODEL for real LLM)");
            Arc::new(mock_llm::MetaAgentMockLLM::new())
        }
    };

    // ── 2. Build parent tool set ────────────────────────────────────────────
    let mut tools = ToolSet::with_defaults(sandbox.clone());

    // ── 3. Load skills if directory exists ──────────────────────────────────
    if std::path::Path::new("skills/").exists() {
        info!("Loading skills from skills/ directory");
        let loader = SkillLoader::new("skills/");
        let parent_tools = Arc::new(tools);
        let skills = loader.load_all(parent_tools.clone(), llm.clone()).await;

        info!("Loaded {} skill(s)", skills.len());

        // Create a new ToolSet with parent tools and skills
        let mut tools_with_skills = ToolSet::with_defaults(sandbox);
        for skill in skills {
            let skill_name = skill.description().name.clone();
            tools_with_skills.register(skill)?;
            info!("Registered skill: {}", skill_name);
        }
        tools = tools_with_skills;
    } else {
        info!("No skills/ directory found, running without skills");
    }

    // ── 4. Build control loop ───────────────────────────────────────────────
    let control_loop = ControlLoop::builder()
        .store(store.clone())
        .llm(llm)
        .tools(Arc::new(tools))
        .system_prompt("You are a meta agent. Use skills to delegate complex subtasks.")
        .build();

    // ── 5. Create session and add user message ─────────────────────────────
    let session_id = store.create_session().await?;
    info!(?session_id, "session created");

    store
        .append_event(
            session_id,
            EventPayload::UserMessage {
                content: "Research the latest developments in Rust async runtimes.".to_string(),
            },
            Actor::System,
            None,
        )
        .await?;

    // ── 6. Run the agent ────────────────────────────────────────────────────
    let answer = control_loop.run(session_id).await?;
    println!("\n=== Meta Agent Answer ===");
    println!("{}", answer);

    // ── 7. Inspect session tree ─────────────────────────────────────────────
    let children = store.child_sessions(session_id).await?;
    println!("\n=== Session Tree ===");
    println!("Parent session: {}", session_id);
    println!("Child sessions: {}", children.len());
    for child in &children {
        println!("  - Skill '{}': {}", child.skill_name, child.session_id);

        // Print child session events
        let child_events = child
            .store
            .get_events(child.session_id, &EventFilter::default())
            .await?;
        println!("    Events: {}", child_events.len());
    }

    // ── 8. Print full event log ─────────────────────────────────────────────
    let events = store
        .get_events(session_id, &EventFilter::default())
        .await?;
    println!(
        "\n=== Parent Session Event Log ({} events) ===",
        events.len()
    );
    for event in &events {
        println!("  [{:?}] {:?}", event.actor, event.payload);
    }

    Ok(())
}
