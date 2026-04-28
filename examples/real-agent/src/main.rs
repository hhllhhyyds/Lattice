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
//! # Codex CLI login
//! LATTICE_LLM_PROVIDER=codex \
//!   LATTICE_MODEL=gpt-5.5 \
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
    let api_base = env::var("LATTICE_API_BASE").ok();
    let model = env::var("LATTICE_MODEL").unwrap_or_else(|_| match provider.as_str() {
        "anthropic" => "claude-sonnet-4-20250514".into(),
        "codex" => "gpt-5.5".into(),
        _ => "gpt-4o".into(),
    });

    info!(provider = %provider, model = %model, "creating LLM client");

    // Create the LLM client based on provider.
    let llm: Arc<dyn LLMClient> = match provider.as_str() {
        "anthropic" => {
            let api_key = env::var("LATTICE_API_KEY").context("LATTICE_API_KEY not set")?;
            let mut client = lattice::llm_anthropic::AnthropicClient::new(&api_key, &model);
            if let Some(base) = api_base {
                client = client.with_base_url(base);
            }
            Arc::new(client)
        }
        "codex" => {
            let _ = api_base;
            Arc::new(lattice::llm_openai::CodexCliClient::new(&model))
        }
        _ => {
            let api_key = env::var("LATTICE_API_KEY").context("LATTICE_API_KEY not set")?;
            let mut client = lattice::llm_openai::OpenAIClient::new(api_key, &model);
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
