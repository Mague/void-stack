mod server;

use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

use server::DevLaunchMcp;

#[tokio::main]
async fn main() -> Result<()> {
    // Tracing must go to stderr — stdout is the MCP JSON-RPC channel
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("DevLaunch MCP server starting");

    let service = DevLaunchMcp::new()
        .serve(stdio())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start MCP server: {}", e))?;

    service.waiting().await?;

    tracing::info!("DevLaunch MCP server stopped");
    Ok(())
}
