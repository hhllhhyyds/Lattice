//! Real agent example — end-to-end with a real LLM.
//!
//! All env vars must be set explicitly; there are no defaults.
//!
//! Usage:
//!
//! ```bash
//! # OpenAI-compatible (including MiniMax, vLLM, Ollama, etc.)
//! LATTICE_LLM_PROVIDER=openai \
//!   LATTICE_API_KEY=sk-xxx \
//!   LATTICE_OPENAI_API_BASE=https://api.minimax.chat/v1 \
//!   LATTICE_OPENAI_MODEL=MiniMax-M2.7 \
//!   cargo run -p real-agent -- "List files in the current directory"
//!
//! # Anthropic
//! LATTICE_LLM_PROVIDER=anthropic \
//!   LATTICE_API_KEY=sk-ant-xxx \
//!   LATTICE_ANTHROPIC_API_BASE=https://api.anthropic.com \
//!   LATTICE_MODEL=claude-sonnet-4-20250514 \
//!   cargo run -p real-agent -- "What is the current date?"
//! ```

use std::env;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use lattice::core::{Actor, EventFilter, EventPayload, LLMClient, SessionStore, ToolExecutor};
use lattice::runtime::ControlLoop;
use lattice::sandbox_local::LocalSandbox;
use lattice::skill::SkillLoader;
use lattice::store_memory::MemoryStore;
use lattice::tools::ToolSet;
use tracing::{info, warn};

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
    dotenvy::dotenv().ok();

    // Read configuration from environment variables. Every value must be set
    // explicitly — there are no built-in defaults, so a misconfigured `.env`
    // fails fast at startup instead of silently calling the wrong endpoint.
    let provider = env::var("LATTICE_LLM_PROVIDER").context("LATTICE_LLM_PROVIDER not set")?;
    let api_key = env::var("LATTICE_API_KEY").context("LATTICE_API_KEY not set")?;
    let (api_base, model) = match provider.as_str() {
        "anthropic" => {
            let base = env::var("LATTICE_ANTHROPIC_API_BASE")
                .or_else(|_| env::var("LATTICE_API_BASE"))
                .context(
                    "LATTICE_ANTHROPIC_API_BASE (or LATTICE_API_BASE) not set for anthropic provider",
                )?;
            let model = env::var("LATTICE_MODEL")
                .context("LATTICE_MODEL not set for anthropic provider")?;
            (base, model)
        }
        "openai" | "openai-compatible" => {
            let base = env::var("LATTICE_OPENAI_API_BASE")
                .or_else(|_| env::var("LATTICE_API_BASE"))
                .context(
                    "LATTICE_OPENAI_API_BASE (or LATTICE_API_BASE) not set for openai provider",
                )?;
            let model = env::var("LATTICE_OPENAI_MODEL")
                .or_else(|_| env::var("LATTICE_MODEL"))
                .context("LATTICE_OPENAI_MODEL (or LATTICE_MODEL) not set for openai provider")?;
            (base, model)
        }
        other => {
            bail!("unsupported LATTICE_LLM_PROVIDER '{other}' (expected 'anthropic' or 'openai')")
        }
    };

    info!(provider = %provider, model = %model, api_base = %api_base, "creating LLM client");

    // Create the LLM client based on provider.
    let llm: Arc<dyn LLMClient> = match provider.as_str() {
        "anthropic" => Arc::new(
            lattice::llm_anthropic::AnthropicClient::new(&api_key, &model).with_base_url(api_base),
        ),
        _ => Arc::new(
            lattice::llm_openai::OpenAIClient::new(&api_key, &model).with_base_url(api_base),
        ),
    };

    // Assemble the agent components.
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::new());
    let sandbox: Arc<dyn lattice::core::Sandbox> = Arc::new(LocalSandbox::new());

    // Phase 1: base tools — these are inherited by skill child loops.
    let mut base_tools = ToolSet::with_defaults(Arc::clone(&sandbox));
    let mcp_manager = lattice_mcp::load_mcp_manager_from_env()
        .await
        .map_err(anyhow::Error::msg)?;
    if let Some(manager) = &mcp_manager {
        lattice_mcp::register_mcp_tools(&mut base_tools, Arc::clone(manager))?;
        for snapshot in manager.list_status_snapshots() {
            match snapshot.state {
                lattice_mcp::McpConnectionState::Connected => {
                    info!(
                        server = %snapshot.name,
                        transport = %snapshot.transport,
                        tool_count = snapshot.tool_count,
                        resource_count = snapshot.resource_count,
                        "MCP server connected"
                    );
                }
                lattice_mcp::McpConnectionState::Failed => {
                    warn!(
                        server = %snapshot.name,
                        transport = %snapshot.transport,
                        detail = %snapshot.detail,
                        "MCP server failed to connect"
                    );
                }
                lattice_mcp::McpConnectionState::Pending => {
                    info!(server = %snapshot.name, "MCP server pending");
                }
            }
        }
    }
    let parent_tools = Arc::new(base_tools);

    // Load skills from ./skills/ — each SkillTool spawns a child ControlLoop
    // that inherits parent_tools (minus any allowed-tools restrictions).
    let skill_loader = SkillLoader::new("./skills");
    let skill_tools = skill_loader
        .load_all(Arc::clone(&parent_tools), Arc::clone(&llm))
        .await;
    info!(count = skill_tools.len(), "skills loaded");

    // Phase 2: agent tools = base tools + loaded skills.
    let mut agent_tools = ToolSet::with_defaults(Arc::clone(&sandbox));
    if let Some(manager) = &mcp_manager {
        lattice_mcp::register_mcp_tools(&mut agent_tools, Arc::clone(manager))?;
    }
    for skill in skill_tools {
        let name = skill.description().name.clone();
        match agent_tools.register(skill) {
            Ok(()) => info!(skill = %name, "skill registered"),
            Err(e) => warn!(skill = %name, error = %e, "skill registration failed"),
        }
    }
    let tools = Arc::new(agent_tools);
    let control_loop = ControlLoop::with_options(
        store.clone(),
        llm,
        tools,
        "You are a helpful agent. You can execute shell commands using the sh tool. \
         Specialized sub-agents are available as skill tools (names prefixed with 'skill:'); \
         use them when the task matches their description. \
         After getting all the information you need, provide a clear final answer."
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
            EventPayload::Thinking { reasoning, .. } => {
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
            EventPayload::ToolCallError { error, error_kind } => {
                format!(
                    "ToolError: kind={} message={}",
                    error_kind.as_str(),
                    truncate(error, 80)
                )
            }
            EventPayload::FinalAnswer { answer } => {
                format!("FinalAnswer: {}", truncate(answer, 80))
            }
            EventPayload::SkillInvoked {
                skill_name,
                child_session_id,
            } => {
                format!("SkillInvoked: {skill_name} child={child_session_id}")
            }
            EventPayload::SkillCompleted {
                skill_name,
                child_session_id,
            } => {
                format!("SkillCompleted: {skill_name} child={child_session_id}")
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
/// Ensures truncation happens at a valid UTF-8 character boundary.
fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', "\\n");
    if s.len() > max {
        // Find the last character boundary that doesn't exceed max bytes
        let truncate_at = s
            .char_indices()
            .take_while(|(idx, _)| *idx < max)
            .last()
            .map(|(idx, ch)| idx + ch.len_utf8())
            .unwrap_or(0);
        format!("{}...", &s[..truncate_at])
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_ascii() {
        assert_eq!(truncate("hello world", 5), "hello...");
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_utf8() {
        // Chinese characters (3 bytes each in UTF-8)
        assert_eq!(truncate("你好世界", 6), "你好...");
        assert_eq!(truncate("文件", 3), "文...");

        // Mixed ASCII and Chinese - this string is shorter than 80 bytes
        let input = r"以下是当前目录 `D:\GKXTwork\Lattice` 下的文件列表";
        let result = truncate(input, 80);
        // Since input is less than 80 bytes, it should not be truncated
        assert_eq!(result, input);

        // Long Chinese string that will be truncated
        let long_input =
            "以下是当前目录下的文件列表：文件1、文件2、文件3、文件4、文件5、文件6、文件7、文件8";
        let result = truncate(long_input, 50);
        assert!(result.ends_with("..."));
        // Verify it's valid UTF-8 and doesn't panic
        assert!(result.chars().count() > 0);
        // Verify the truncation happened at a character boundary (no panic = success)
        assert!(!result.is_empty() && result.len() < long_input.len());
    }

    #[test]
    fn test_truncate_newlines() {
        assert_eq!(truncate("hello\nworld", 20), "hello\\nworld");
        assert_eq!(truncate("line1\nline2\nline3", 10), "line1\\nlin...");
    }
}
