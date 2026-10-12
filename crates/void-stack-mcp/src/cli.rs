//! Command-line surface of the MCP server.
//!
//! Without arguments the server keeps its historical behaviour: MCP over
//! stdio. `--http <addr>` switches the very same [`crate::server::VoidStackMcp`]
//! to the streamable-HTTP transport so remote clients (Claude Code in WSL,
//! another machine over Tailscale) can reach it.

use std::net::{IpAddr, Ipv6Addr, SocketAddr, ToSocketAddrs};

use anyhow::{Context, Result, anyhow};
use clap::Parser;

/// Address used when only a port is given (`--http 7400`).
const DEFAULT_HTTP_HOST: &str = "127.0.0.1";

#[derive(Parser, Debug, Clone, PartialEq, Eq)]
#[command(
    name = "void-stack-mcp",
    version,
    about = "MCP server for Void Stack (stdio by default, --http for streamable HTTP)"
)]
pub struct Cli {
    /// Serve MCP over streamable HTTP on this address instead of stdio.
    /// Accepts `host:port`, `ip:port` or a bare port (`7400` →
    /// `127.0.0.1:7400`).
    #[arg(long, value_name = "ADDR")]
    pub http: Option<String>,
}

/// Resolve a `--http` value into a bind address.
///
/// A bare port binds loopback on purpose: the safe default should be the
/// short one to type.
pub fn parse_listen_addr(value: &str) -> Result<SocketAddr> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("--http needs an address, e.g. 127.0.0.1:7400"));
    }

    // Bare port: `--http 7400`
    if let Ok(port) = value.parse::<u16>() {
        return parse_listen_addr(&format!("{DEFAULT_HTTP_HOST}:{port}"));
    }

    // `ip:port` first (no DNS), then host names through the resolver.
    if let Ok(addr) = value.parse::<SocketAddr>() {
        return Ok(addr);
    }

    let mut resolved = value
        .to_socket_addrs()
        .with_context(|| format!("Cannot resolve --http address '{value}'"))?;
    resolved
        .next()
        .ok_or_else(|| anyhow!("--http address '{value}' resolved to no address"))
}

/// Whether binding here keeps the (unauthenticated) server on a trusted
/// network: loopback, or one of the Tailscale ranges.
pub fn is_trusted_bind(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            // 100.64.0.0/10 — CGNAT range Tailscale hands out.
            v4.is_loopback() || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => v6.is_loopback() || is_tailscale_ula(v6),
    }
}

/// fd7a:115c:a1e0::/48 — Tailscale's IPv6 ULA prefix.
fn is_tailscale_ula(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
}

/// Warning shown before serving on a non-trusted address, or `None` when
/// the bind is fine. The HTTP transport carries no authentication, so the
/// only thing protecting it is the network it listens on.
pub fn untrusted_bind_warning(addr: &SocketAddr) -> Option<String> {
    if is_trusted_bind(addr.ip()) {
        return None;
    }
    Some(format!(
        "Serving MCP on {addr}: this address is neither loopback nor a Tailscale range \
         and the HTTP transport has no authentication. Anyone who can reach it gets full \
         access to your projects; restrict it to localhost or your tailnet."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("valid args")
    }

    #[test]
    fn no_flag_means_stdio() {
        assert_eq!(parse(&["void-stack-mcp"]).http, None);
    }

    #[test]
    fn http_flag_captures_the_address() {
        assert_eq!(
            parse(&["void-stack-mcp", "--http", "0.0.0.0:7400"])
                .http
                .as_deref(),
            Some("0.0.0.0:7400")
        );
    }

    #[test]
    fn http_flag_requires_a_value() {
        assert!(Cli::try_parse_from(["void-stack-mcp", "--http"]).is_err());
    }

    #[test]
    fn unknown_flags_are_rejected() {
        assert!(Cli::try_parse_from(["void-stack-mcp", "--sse"]).is_err());
    }

    #[test]
    fn parses_ipv4_socket_addr() {
        let addr = parse_listen_addr("127.0.0.1:7400").unwrap();
        assert_eq!(addr, SocketAddr::from(([127, 0, 0, 1], 7400)));
    }

    #[test]
    fn parses_wildcard_addr() {
        let addr = parse_listen_addr("0.0.0.0:7400").unwrap();
        assert_eq!(addr, SocketAddr::from(([0, 0, 0, 0], 7400)));
    }

    #[test]
    fn parses_ipv6_socket_addr() {
        let addr = parse_listen_addr("[::1]:7400").unwrap();
        assert_eq!(addr.ip(), IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(addr.port(), 7400);
    }

    #[test]
    fn bare_port_binds_loopback() {
        let addr = parse_listen_addr("7400").unwrap();
        assert_eq!(addr, SocketAddr::from(([127, 0, 0, 1], 7400)));
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_eq!(
            parse_listen_addr("  127.0.0.1:7400 ").unwrap(),
            SocketAddr::from(([127, 0, 0, 1], 7400))
        );
    }

    #[test]
    fn resolves_localhost_by_name() {
        let addr = parse_listen_addr("localhost:7400").unwrap();
        assert!(addr.ip().is_loopback());
        assert_eq!(addr.port(), 7400);
    }

    #[test]
    fn empty_value_is_an_error() {
        assert!(parse_listen_addr("   ").is_err());
    }

    #[test]
    fn missing_port_is_an_error() {
        assert!(parse_listen_addr("127.0.0.1").is_err());
    }

    #[test]
    fn loopback_and_tailscale_are_trusted() {
        assert!(is_trusted_bind(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(is_trusted_bind(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(is_trusted_bind(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        assert!(is_trusted_bind(IpAddr::V4(Ipv4Addr::new(
            100, 127, 255, 254
        ))));
        assert!(is_trusted_bind(IpAddr::V6(Ipv6Addr::new(
            0xfd7a, 0x115c, 0xa1e0, 0, 0, 0, 0, 1
        ))));
    }

    #[test]
    fn lan_and_wildcard_are_not_trusted() {
        assert!(!is_trusted_bind(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        assert!(!is_trusted_bind(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))));
        // Just outside the CGNAT range on either side.
        assert!(!is_trusted_bind(IpAddr::V4(Ipv4Addr::new(
            100, 63, 255, 255
        ))));
        assert!(!is_trusted_bind(IpAddr::V4(Ipv4Addr::new(100, 128, 0, 1))));
        assert!(!is_trusted_bind(IpAddr::V6(Ipv6Addr::new(
            0xfd7a, 0x115c, 0xa1e1, 0, 0, 0, 0, 1
        ))));
    }

    #[test]
    fn warning_only_fires_for_untrusted_binds() {
        assert!(untrusted_bind_warning(&SocketAddr::from(([127, 0, 0, 1], 7400))).is_none());
        assert!(untrusted_bind_warning(&SocketAddr::from(([100, 100, 5, 5], 7400))).is_none());

        let warning = untrusted_bind_warning(&SocketAddr::from(([0, 0, 0, 0], 7400)))
            .expect("wildcard bind should warn");
        assert!(warning.contains("0.0.0.0:7400"));
        assert!(warning.contains("no authentication"));
    }
}
