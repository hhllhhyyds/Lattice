//! Lattice HTTP API server entry point.

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use lattice_server::{default_llm_summary, new_state_from_env, router, DefaultLlmSummary};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // Initialize tracing.
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("info".parse()?))
        .try_init()
        .ok();

    // Read listen address from environment.
    let host = env::var("LATTICE_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port = env::var("LATTICE_PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse::<u16>()?;
    let addr: SocketAddr = format!("{host}:{port}").parse()?;

    // Create default session store (MemoryStore for MVP).
    let store: Arc<dyn lattice_core::SessionStore> =
        Arc::new(lattice_store_memory::MemoryStore::new());

    // Build router and state.
    let state = new_state_from_env(store)
        .await
        .map_err(anyhow::Error::msg)?;
    if let Some(manager) = &state.mcp_manager {
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
                    tracing::warn!(
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
    let app = router(state);

    // Resolve the LLM default config for the banner / structured log.
    // Missing values are reported as `(not set)` so the operator sees what's
    // unconfigured at a glance — the request-time `create_llm_client` will
    // error explicitly when a request comes in without overriding the gap.
    let llm = default_llm_summary();
    info!(
        provider = llm.provider.as_deref().unwrap_or("(not set)"),
        model = llm.model.as_deref().unwrap_or("(not set)"),
        api_base = llm.api_base.as_deref().unwrap_or("(not set)"),
        "default LLM configuration"
    );

    // Print startup banner.
    print_banner(addr, &llm);

    // Start the server with graceful shutdown.
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("server shut down gracefully");
    Ok(())
}

/// Waits for a shutdown signal (Ctrl-C on all platforms, SIGTERM on Unix).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => { info!("received SIGINT, shutting down"); }
        () = sigterm => { info!("received SIGTERM, shutting down"); }
    }
}

fn print_banner(addr: SocketAddr, llm: &DefaultLlmSummary) {
    let provider = llm.provider.as_deref().unwrap_or("(not set)");
    let model = llm.model.as_deref().unwrap_or("(not set)");
    let api_base = llm.api_base.as_deref().unwrap_or("(not set)");

    println!();
    println!("  ██╗      █████╗ ████████╗████████╗██╗ ██████╗███████╗");
    println!("  ██║     ██╔══██╗╚══██╔══╝╚══██╔══╝██║██╔════╝██╔════╝");
    println!("  ██║     ███████║   ██║      ██║   ██║██║     █████╗  ");
    println!("  ██║     ██╔══██║   ██║      ██║   ██║██║     ██╔══╝  ");
    println!("  ███████╗██║  ██║   ██║      ██║   ██║╚██████╗███████╗");
    println!("  ╚══════╝╚═╝  ╚═╝   ╚═╝      ╚═╝   ╚═╝ ╚═════╝╚══════╝");
    println!();
    println!("  Version:  {}", env!("CARGO_PKG_VERSION"));
    println!("  Address:  http://{addr}");
    println!("  Features: {}", enabled_features_summary());
    println!("  Provider: {provider}");
    println!("  Model:    {model}");
    println!("  API Base: {api_base}");
    println!("  Status:   {}", "ready".bright_green());
    println!();
}

fn enabled_features_summary() -> String {
    let anthropic = if cfg!(feature = "anthropic") {
        "anthropic"
    } else {
        "-anthropic"
    };
    let openai = if cfg!(feature = "openai") {
        "openai"
    } else {
        "-openai"
    };
    format!("{anthropic}, {openai}")
}

// Helper extension trait for colored output on supported terminals.
trait Colored {
    fn bright_green(&self) -> String;
}

impl Colored for str {
    fn bright_green(&self) -> String {
        format!("\x1b[1;32m{self}\x1b[0m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enabled_features_summary() {
        let summary = enabled_features_summary();
        // Should contain "anthropic" or "-anthropic" and "openai" or "-openai"
        assert!(summary.contains("anthropic"));
        assert!(summary.contains("openai"));
    }

    #[test]
    fn test_bright_green() {
        let colored = "ready".bright_green();
        assert!(colored.contains("ready"));
        assert!(colored.contains("\x1b[1;32m"));
        assert!(colored.contains("\x1b[0m"));
    }

    #[test]
    fn test_bright_green_empty_string() {
        let colored = "".bright_green();
        assert_eq!(colored, "\x1b[1;32m\x1b[0m");
    }

    #[test]
    fn test_enabled_features_summary_contains_both() {
        let summary = enabled_features_summary();
        // Format: "{anthropic}, {openai}" where each is "name" or "-name"
        let parts: Vec<&str> = summary.split(", ").collect();
        assert_eq!(parts.len(), 2);
    }
}
