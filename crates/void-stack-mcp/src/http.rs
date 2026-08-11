//! Streamable-HTTP transport for the MCP server.
//!
//! Same [`VoidStackMcp`] service as stdio, mounted on an axum router so
//! remote clients can talk to it over HTTP. One process serves stdio *or*
//! HTTP — never both.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio::net::TcpListener;

use crate::server::VoidStackMcp;

/// Router exposing the MCP endpoint.
///
/// The service is the router's fallback, so `/` and `/mcp` (and anything
/// else) hit the same handler — clients disagree on whether the URL
/// carries a path, and both spellings should just work.
pub fn router() -> axum::Router {
    let service: StreamableHttpService<VoidStackMcp, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(VoidStackMcp::new()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );

    axum::Router::new().fallback_service(service)
}

/// Serve MCP over HTTP on an already-bound listener until `shutdown`
/// resolves. Split out from [`serve_http`] so tests can drive an ephemeral
/// port and stop the server deterministically.
pub async fn serve(
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    axum::serve(listener, router())
        .with_graceful_shutdown(shutdown)
        .await
        .context("MCP HTTP server failed")
}

/// Bind `addr` and serve MCP over HTTP until Ctrl+C.
pub async fn serve_http(addr: SocketAddr) -> Result<()> {
    if let Some(warning) = crate::cli::untrusted_bind_warning(&addr) {
        tracing::warn!("{warning}");
    }

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Cannot bind {addr}"))?;
    let local = listener.local_addr().unwrap_or(addr);

    tracing::info!(address = %local, "MCP streamable HTTP listening (endpoint: / or /mcp)");

    serve(listener, async {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutdown signal received");
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tokio::sync::oneshot;

    const INIT_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"vb29-test","version":"0"}}}"#;

    /// A server on an ephemeral port plus the handle that stops it.
    struct TestServer {
        addr: SocketAddr,
        stop: Option<oneshot::Sender<()>>,
        task: tokio::task::JoinHandle<Result<()>>,
    }

    impl TestServer {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("local addr");
            let (stop, stopped) = oneshot::channel();
            let task = tokio::spawn(serve(listener, async {
                stopped.await.ok();
            }));
            Self {
                addr,
                stop: Some(stop),
                task,
            }
        }

        fn url(&self, path: &str) -> String {
            format!("http://{}{path}", self.addr)
        }

        async fn shutdown(mut self) {
            self.stop.take().expect("stop channel").send(()).ok();
            self.task.await.expect("server task").expect("clean exit");
        }
    }

    /// POST a JSON-RPC message, optionally inside a session.
    async fn post(
        client: &reqwest::Client,
        url: &str,
        session: Option<&str>,
        body: String,
    ) -> reqwest::Response {
        let mut request = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(body);
        if let Some(session) = session {
            request = request.header("mcp-session-id", session);
        }
        request.send().await.expect("request sent")
    }

    /// The transport answers POSTs as SSE; pull the single JSON payload out
    /// of the `data:` lines, skipping the priming event.
    fn json_from_sse(body: &str) -> Value {
        body.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
            .find(|value| value.get("result").is_some() || value.get("error").is_some())
            .unwrap_or_else(|| panic!("no JSON-RPC payload in SSE body: {body}"))
    }

    /// Initialize a session and return its id.
    async fn initialize(client: &reqwest::Client, server: &TestServer) -> (String, Value) {
        let response = post(client, &server.url("/mcp"), None, INIT_BODY.to_string()).await;
        assert_eq!(response.status(), 200);
        let session = response
            .headers()
            .get("mcp-session-id")
            .expect("session id header")
            .to_str()
            .expect("ascii session id")
            .to_string();
        let payload = json_from_sse(&response.text().await.expect("body"));

        // The spec wants the initialized notification before real calls.
        let notified = post(
            client,
            &server.url("/mcp"),
            Some(&session),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
        )
        .await;
        assert!(notified.status().is_success());

        (session, payload)
    }

    #[tokio::test]
    async fn initialize_over_http_returns_server_info() {
        let server = TestServer::start().await;
        let client = reqwest::Client::new();

        let (_session, payload) = initialize(&client, &server).await;

        let info = &payload["result"]["serverInfo"];
        assert!(
            info["name"].is_string(),
            "expected serverInfo in {payload:#}"
        );
        assert!(payload["result"]["capabilities"]["tools"].is_object());

        server.shutdown().await;
    }

    #[tokio::test]
    async fn http_lists_the_same_tools_as_the_in_process_router() {
        let server = TestServer::start().await;
        let client = reqwest::Client::new();
        let (session, _) = initialize(&client, &server).await;

        let response = post(
            &client,
            &server.url("/mcp"),
            Some(&session),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}).to_string(),
        )
        .await;
        assert_eq!(response.status(), 200);
        let payload = json_from_sse(&response.text().await.expect("body"));

        let mut over_http: Vec<String> = payload["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("no tools array in {payload:#}"))
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name").to_string())
            .collect();
        over_http.sort();

        // Same registry the stdio transport serves.
        let in_process = VoidStackMcp::tool_names();

        assert!(!in_process.is_empty(), "tool router should not be empty");
        assert_eq!(over_http, in_process);
        assert!(over_http.iter().any(|name| name == "list_projects"));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn root_path_serves_the_same_endpoint_as_mcp() {
        let server = TestServer::start().await;
        let client = reqwest::Client::new();

        let response = post(&client, &server.url("/"), None, INIT_BODY.to_string()).await;

        assert_eq!(response.status(), 200);
        assert!(response.headers().contains_key("mcp-session-id"));
        let payload = json_from_sse(&response.text().await.expect("body"));
        assert!(payload["result"]["serverInfo"]["name"].is_string());

        server.shutdown().await;
    }

    #[tokio::test]
    async fn calls_without_a_session_are_rejected() {
        let server = TestServer::start().await;
        let client = reqwest::Client::new();

        let response = post(
            &client,
            &server.url("/mcp"),
            None,
            json!({"jsonrpc":"2.0","id":9,"method":"tools/list","params":{}}).to_string(),
        )
        .await;

        assert!(
            response.status().is_client_error(),
            "expected 4xx without a session, got {}",
            response.status()
        );

        server.shutdown().await;
    }

    #[tokio::test]
    async fn serve_http_reports_a_bind_failure() {
        // Hold the port, then ask serve_http for the same one.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let error = serve_http(addr).await.expect_err("port is taken");

        assert!(
            error.to_string().contains(&addr.to_string()),
            "error should name the address: {error}"
        );
    }
}
