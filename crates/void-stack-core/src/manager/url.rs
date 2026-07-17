use std::sync::OnceLock;

use regex::Regex;

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|::1)(?::\d+)(?:/[^\s\])\}>"']*)?"#,
        )
        .unwrap()
    })
}

/// Detect URLs like http://localhost:3000 or http://127.0.0.1:8000 from a log line.
pub(crate) fn detect_url(line: &str) -> Option<String> {
    // Strip ANSI codes first (Vite, Next.js, etc. colorize URLs)
    let clean = crate::log_filter::strip_ansi(line);

    url_regex().find(&clean).map(|m| {
        let url = m.as_str().to_string();
        // Normalize 0.0.0.0 to localhost for browser use
        url.replace("0.0.0.0", "localhost")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_url_localhost() {
        assert_eq!(
            detect_url("Server running at http://localhost:3000"),
            Some("http://localhost:3000".to_string())
        );
    }

    #[test]
    fn test_detect_url_127() {
        assert_eq!(
            detect_url("Listening on http://127.0.0.1:8000/api"),
            Some("http://127.0.0.1:8000/api".to_string())
        );
    }

    #[test]
    fn test_detect_url_0000() {
        assert_eq!(
            detect_url("  ->  Local:   http://0.0.0.0:5173/"),
            Some("http://localhost:5173/".to_string())
        );
    }

    #[test]
    fn test_detect_url_none() {
        assert_eq!(detect_url("Starting compilation..."), None);
    }

    #[test]
    fn test_detect_url_https() {
        assert_eq!(
            detect_url("Ready on https://localhost:3000"),
            Some("https://localhost:3000".to_string())
        );
    }

    #[test]
    fn test_detect_url_ansi_colored() {
        // Vite wraps URLs in ANSI color codes
        assert_eq!(
            detect_url("  ->  Local:   \x1b[36mhttp://localhost:5173/\x1b[0m"),
            Some("http://localhost:5173/".to_string())
        );
    }

    #[test]
    fn test_strip_ansi_via_log_filter() {
        assert_eq!(
            crate::log_filter::strip_ansi("\x1b[36mhello\x1b[0m world"),
            "hello world"
        );
        assert_eq!(
            crate::log_filter::strip_ansi("no codes here"),
            "no codes here"
        );
    }

    #[test]
    fn test_detect_url_ipv6_loopback() {
        // The [::1] literal host is recognised (port is still required).
        assert_eq!(
            detect_url("Now serving http://::1:9000/health"),
            Some("http://::1:9000/health".to_string())
        );
    }

    #[test]
    fn test_detect_url_requires_a_port() {
        // A portless localhost URL is intentionally not matched.
        assert_eq!(detect_url("Open http://localhost to continue"), None);
    }

    #[test]
    fn test_detect_url_stops_at_trailing_delimiter() {
        // The path character class excludes closing brackets/quotes, so a URL
        // wrapped in parentheses does not swallow the trailing ')'.
        assert_eq!(
            detect_url("(see http://localhost:8080/docs)"),
            Some("http://localhost:8080/docs".to_string())
        );
    }

    #[test]
    fn test_detect_url_returns_first_match() {
        // When several URLs appear, the first is returned.
        assert_eq!(
            detect_url("http://localhost:3000 and http://127.0.0.1:4000"),
            Some("http://localhost:3000".to_string())
        );
    }

    #[test]
    fn test_detect_url_ignores_non_loopback_host() {
        // Only loopback hosts are detected; public hosts are ignored.
        assert_eq!(detect_url("Deployed to http://example.com:443/"), None);
    }
}
