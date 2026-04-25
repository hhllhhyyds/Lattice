//! Lattice HTTP API server entry point.

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use lattice_server::{new_state, router};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    let state = new_state(store);
    let app = router(state);

    // Print startup banner.
    print_banner(addr);

    // Start the server.
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "server listening");
    axum::serve(listener, app).await?;

    Ok(())
}

fn print_banner(addr: SocketAddr) {
    println!();
    println!("  ██╗   ██╗███╗   ██╗██╗  ██╗███████╗███╗   ██╗");
    println!("  ██║   ██║████╗  ██║██║ ██╔╝██╔════╝████╗  ██║");
    println!("  ██║   ██║██╔██╗ ██║█████╔╝ █████╗  ██╔██╗ ██║");
    println!("  ██║   ██║██║╚██╗██║██╔═██╗ ██╔══╝  ██║╚██╗██║");
    println!("  ╚██████╔╝██║ ╚████║██║  ██╗███████╗██║ ╚████║");
    println!("   ╚═════╝ ╚═╝  ╚═══╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝");
    println!();
    println!("  Version: {}", env!("CARGO_PKG_VERSION"));
    println!("  Address: http://{addr}");
    println!("  Features: {}", enabled_features_summary());
    println!("  Status:  {}", "ready".bright_green());
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
