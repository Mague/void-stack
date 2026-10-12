mod cli;
mod http;
mod server;
mod tools;
mod types;

use anyhow::Result;
use clap::Parser;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

use cli::Cli;
use server::VoidStackMcp;

#[tokio::main]
async fn main() -> Result<()> {
    // Tracing must go to stderr — stdout is the MCP JSON-RPC channel
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let args = Cli::parse();

    match args.http.as_deref() {
        Some(value) => {
            let addr = cli::parse_listen_addr(value)?;
            tracing::info!("VoidStack MCP server starting (streamable HTTP)");
            http::serve_http(addr).await?;
        }
        None => serve_stdio().await?,
    }

    tracing::info!("VoidStack MCP server stopped");
    Ok(())
}

async fn serve_stdio() -> Result<()> {
    tracing::info!("VoidStack MCP server starting");

    let service = VoidStackMcp::new()
        .serve(stdio())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start MCP server: {}", e))?;

    service.waiting().await?;
    Ok(())
}
