//! Real agent example — end-to-end with a real LLM.
//!
//! Usage:
//!
//! ```bash
//! # OpenAI-compatible (including MiniMax, vLLM, Ollama, etc.)
//! LATTICE_API_KEY=sk-xxx LATTICE_API_BASE=https://api.minimax.chat/v1 \
//!   LATTICE_MODEL=MiniMax-M2.7 \
//!   cargo run -p real-agent -- "List files in the current directory"
//!
//! # Anthropic
//! LATTICE_LLM_PROVIDER=anthropic LATTICE_API_KEY=sk-ant-xxx \
//!   LATTICE_MODEL=claude-sonnet-4-20250514 \
//!   cargo run -p real-agent -- "What is the current date?"
//! ```

use std::env;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use lattice::core::{Actor, EventFilter, EventPayload, LLMClient, SessionStore};
use lattice::runtime::ControlLoop;
use lattice::sandbox_local::LocalSandbox;
use lattice::store_memory::MemoryStore;
use lattice::tools::ToolSet;
use tracing::info;

fn main() -> Result<()> {
    // Initialize tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Parse command line arguments.
    let args: Vec<String> = env::args().collect();
    let task = if args.len() > 1 {
        args[1..].join(" ")
    } else {
        eprintln!("Usage: real-agent <task>");
        eprintln!("Example: real-agent \"List files in the current directory\"");
        bail!("no task provided");
    };

    // Build the tokio runtime and run.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run(task))
}

async fn run(task: String) -> Result<()> {
    // Read configuration from environment variables.
    let provider = env::var("LATTICE_LLM_PROVIDER").unwrap_or_else(|_| "openai".into());
    let api_key = env::var("LATTICE_API_KEY").context("LATTICE_API_KEY not set")?;
    let api_base = env::var("LATTICE_API_BASE").ok();
    let model = env::var("LATTICE_MODEL").unwrap_or_else(|_| match provider.as_str() {
        "anthropic" => "claude-sonnet-4-20250514".into(),
        _ => "gpt-4o".into(),
    });

    info!(provider = %provider, model = %model, "creating LLM client");

    // Create the LLM client based on provider.
    let llm: Arc<dyn LLMClient> = match provider.as_str() {
        "anthropic" => {
            let mut client = lattice::llm_anthropic::AnthropicClient::new(&api_key, &model);
            if let Some(base) = api_base {
                client = client.with_base_url(base);
            }
            Arc::new(client)
        }
        _ => {
            let mut client = lattice::llm_openai::OpenAIClient::new(&api_key, &model);
            if let Some(base) = api_base {
                client = client.with_base_url(base);
            }
            Arc::new(client)
        }
    };

    // Assemble the agent components.
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::new());
    let sandbox = Arc::new(LocalSandbox::new());
    let tools = Arc::new(ToolSet::with_defaults(sandbox));
    let control_loop = ControlLoop::with_options(
        store.clone(),
        llm,
        tools,
        "You are a helpful agent. You can execute bash commands using the bash tool. \
         Always use the bash tool when you need to interact with the system. \
         After getting the tool result, provide a clear final answer to the user."
            .into(),
        20, // max iterations
    );

    // Create a session and submit the user task.
    let session_id = store.create_session().await?;
    info!(%session_id, "session created");

    store
        .append_event(
            session_id,
            EventPayload::UserMessage {
                content: task.clone(),
            },
            Actor::System,
            None,
        )
        .await?;

    info!(task = %task, "task submitted, starting agent...");

    // Run the control loop.
    let answer = control_loop.run(session_id).await?;

    println!("\n=== Agent Answer ===");
    println!("{answer}");

    // Print the full event log.
    let events = store
        .get_events(session_id, &EventFilter::default())
        .await?;
    println!("\n=== Event Log ({} events) ===", events.len());
    for event in &events {
        let actor = format!("{:?}", event.actor);
        let payload = match &event.payload {
            EventPayload::SessionCreated => "SessionCreated".into(),
            EventPayload::UserMessage { content } => {
                format!("UserMessage: {}", truncate(content, 80))
            }
            EventPayload::Thinking { reasoning } => {
                format!("Thinking: {}", truncate(reasoning, 80))
            }
            EventPayload::ToolCallRequested { tool, params } => {
                format!("ToolCall: {} {}", tool, truncate(&params.to_string(), 60))
            }
            EventPayload::ToolCallResult {
                stdout,
                stderr,
                exit_code,
            } => {
                format!(
                    "ToolResult: exit={} stdout={} stderr={}",
                    exit_code,
                    truncate(stdout, 40),
                    truncate(stderr, 40)
                )
            }
            EventPayload::ToolCallError { error } => {
                format!("ToolError: {}", truncate(error, 80))
            }
            EventPayload::FinalAnswer { answer } => {
                format!("FinalAnswer: {}", truncate(answer, 80))
            }
            EventPayload::StateChange { from, to } => {
                format!("StateChange: {from} → {to}")
            }
        };
        println!("  [{actor}] {payload}");
    }

    Ok(())
}

/// Truncate a string to the given length, appending "..." if truncated.
fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', "\\n");
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s
    }
}
