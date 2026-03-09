mod server;

use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

use server::VoidStackMcp;

#[tokio::main]
async fn main() -> Result<()> {
    // Tracing must go to stderr — stdout is the MCP JSON-RPC channel
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("VoidStack MCP server starting");

    let service = VoidStackMcp::new()
        .serve(stdio())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start MCP server: {}", e))?;

    service.waiting().await?;

    tracing::info!("VoidStack MCP server stopped");
    Ok(())
}
