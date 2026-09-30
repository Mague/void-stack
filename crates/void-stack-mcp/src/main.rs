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
            // El broker de GPU vive aqui porque este es el proceso que esta
            // siempre en pie (la tarea void-stack-mcp). Solo en modo HTTP: el
            // stdio arranca una vez por sesion de Claude y no debe muestrear.
            // Escucha en su PROPIO puerto de loopback (127.0.0.1:7410), no en
            // este: el MCP no tiene autenticacion y admite enlazar por
            // Tailscale, y el broker da ordenes de ceder y de descargar.
            void_stack_gpu::spawn();
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
