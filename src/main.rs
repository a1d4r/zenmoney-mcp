//! ZenMoney MCP server entry point.
//!
//! Reads `ZENMONEY_TOKEN` from the environment, creates a [`ZenMoney`]
//! client backed by [`FileStorage`], performs an initial sync, then
//! serves MCP tools over stdio.

mod params;
mod response;
mod server;

use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use zenmoney_rs::storage::FileStorage;
use zenmoney_rs::zen_money::ZenMoney;

use crate::server::ZenMoneyMcpServer;

/// Server name reported in startup logs.
const SERVER_NAME: &str = env!("CARGO_PKG_NAME");
/// Server version reported in startup logs.
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns the elapsed duration as whole milliseconds, saturating at `u64::MAX`.
fn elapsed_ms(start: std::time::Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Runs the MCP server.
///
/// # Errors
///
/// Returns an error if the token is missing, the client cannot be built,
/// the initial sync fails, or the stdio transport encounters an error.
async fn run() -> Result<(), Box<dyn core::error::Error>> {
    // Initialise tracing to stderr (stdout is used for MCP stdio transport).
    // Default to INFO when `RUST_LOG` is unset so server activity is visible
    // in MCP client logs without extra configuration.
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .init();

    tracing::info!(
        server = SERVER_NAME,
        version = SERVER_VERSION,
        "starting ZenMoney MCP server"
    );

    // Read token from environment.
    let token: String = std::env::var("ZENMONEY_TOKEN")
        .map_err(|_err| "ZENMONEY_TOKEN environment variable is required")?;

    // Create file storage at default XDG location.
    let storage_dir = FileStorage::default_dir()?;
    tracing::info!(
        storage_dir = %storage_dir.display(),
        "initialising file storage"
    );
    let storage = FileStorage::new(storage_dir)?;

    // Build the ZenMoney client.
    let client = ZenMoney::builder().token(token).storage(storage).build()?;

    // Perform initial sync. This is a network call and dominates startup time —
    // logging duration helps diagnose slow first-message responses.
    tracing::info!("performing initial sync");
    let sync_started = std::time::Instant::now();
    let _sync_response = client.sync().await?;
    tracing::info!(
        duration_ms = elapsed_ms(sync_started),
        "initial sync complete"
    );

    // Create MCP server and serve over stdio.
    let mcp_server = ZenMoneyMcpServer::new(client);
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    tracing::info!("starting stdio transport");
    let serve_started = std::time::Instant::now();
    let service = mcp_server.serve(transport).await?;
    tracing::info!(
        duration_ms = elapsed_ms(serve_started),
        "stdio transport ready; entering message loop"
    );

    let _quit_reason = service.waiting().await?;
    tracing::info!("MCP server stopped");

    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        tracing::error!(%err, "fatal error");
        std::process::exit(1);
    }
}
