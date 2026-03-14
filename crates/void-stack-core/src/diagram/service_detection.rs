//! Shared service type detection and port extraction for diagram generators.
//!
//! Extracted from `architecture.rs` and `drawio.rs` to eliminate duplication
//! and reduce cyclomatic complexity via table-driven pattern matching.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum ServiceType {
    Frontend,
    Backend,
    Database,
    Worker,
    Unknown,
}

// ── Command-based detection table ──────────────────────────────────────────
// Each entry: (&[patterns], ServiceType, default_port)
// `default_port = 0` means no default (use extract_port only).

struct CmdRule {
    patterns: &'static [&'static str],
    service_type: ServiceType,
    default_port: u16, // 0 = no default
}

const CMD_RULES: &[CmdRule] = &[
    // Frontend commands
    CmdRule {
        patterns: &["npm run dev", "yarn dev", "vite", "next"],
        service_type: ServiceType::Frontend,
        default_port: 3000,
    },
    CmdRule {
        patterns: &["flutter run", "flutter serve"],
        service_type: ServiceType::Frontend,
        default_port: 0,
    },
    // Backend commands
    CmdRule {
        patterns: &["uvicorn", "gunicorn", "flask"],
        service_type: ServiceType::Backend,
        default_port: 8000,
    },
    CmdRule {
        patterns: &["django", "manage.py"],
        service_type: ServiceType::Backend,
        default_port: 8000,
    },
    CmdRule {
        patterns: &["cargo run", "go run"],
        service_type: ServiceType::Backend,
        default_port: 0,
    },
    CmdRule {
        patterns: &["dart run"],
        service_type: ServiceType::Backend,
        default_port: 0,
    },
    // Worker commands
    CmdRule {
        patterns: &["celery", "worker"],
        service_type: ServiceType::Worker,
        default_port: 0,
    },
    // Docker — unknown type
    CmdRule {
        patterns: &["docker compose"],
        service_type: ServiceType::Unknown,
        default_port: 0,
    },
];

// Python commands get special handling (starts_with instead of contains)
const PYTHON_PREFIXES: &[&str] = &["python ", "python3 "];
const PYTHON_DEFAULT_PORT: u16 = 8000;

// ── Framework detection tables for package.json ────────────────────────────

const NODE_FRONTEND_MARKERS: &[&str] =
    &["react", "vue", "svelte", "next", "vite", "nuxt", "angular"];
const NODE_BACKEND_MARKERS: &[&str] = &["express", "fastify", "nest", "koa", "hapi"];

// ── Framework detection tables for pubspec.yaml ────────────────────────────

const DART_FRONTEND_MARKERS: &[&str] = &["flutter:"];
const DART_BACKEND_MARKERS: &[&str] = &["shelf", "dart_frog", "grpc"];

/// Detect the service type and optional port from the command string and
/// directory contents. Uses table-driven matching to keep complexity low.
pub fn detect_service_info(dir: &Path, command: &str) -> (ServiceType, Option<u16>) {
    let cmd_lower = command.to_lowercase();

    // 1. Match command patterns (table-driven)
    for rule in CMD_RULES {
        if rule.patterns.iter().any(|p| cmd_lower.contains(p)) {
            let port = extract_port(&cmd_lower).or(if rule.default_port > 0 {
                Some(rule.default_port)
            } else {
                None
            });
            return (rule.service_type, port);
        }
    }

    // 2. Python special case (starts_with)
    if PYTHON_PREFIXES.iter().any(|p| cmd_lower.starts_with(p)) {
        return (
            ServiceType::Backend,
            extract_port(&cmd_lower).or(Some(PYTHON_DEFAULT_PORT)),
        );
    }

    // 3. Detect from directory contents
    if let Some(result) = detect_from_package_json(dir) {
        return result;
    }
    if dir.join("requirements.txt").exists() || dir.join("pyproject.toml").exists() {
        return (ServiceType::Backend, Some(8000));
    }
    if let Some(result) = detect_from_pubspec(dir) {
        return result;
    }

    (ServiceType::Unknown, None)
}

/// Check package.json for frontend/backend framework markers.
fn detect_from_package_json(dir: &Path) -> Option<(ServiceType, Option<u16>)> {
    let pkg = dir.join("package.json");
    if !pkg.exists() {
        return None;
    }
    if let Ok(content) = std::fs::read_to_string(&pkg) {
        let lower = content.to_lowercase();
        if NODE_FRONTEND_MARKERS.iter().any(|m| lower.contains(m)) {
            return Some((ServiceType::Frontend, Some(3000)));
        }
        if NODE_BACKEND_MARKERS.iter().any(|m| lower.contains(m)) {
            return Some((ServiceType::Backend, Some(3000)));
        }
    }
    Some((ServiceType::Frontend, Some(3000)))
}

/// Check pubspec.yaml for Flutter/Dart framework markers.
fn detect_from_pubspec(dir: &Path) -> Option<(ServiceType, Option<u16>)> {
    let pubspec = dir.join("pubspec.yaml");
    if !pubspec.exists() {
        return None;
    }
    if let Ok(content) = std::fs::read_to_string(&pubspec) {
        let lower = content.to_lowercase();
        if DART_FRONTEND_MARKERS.iter().any(|m| lower.contains(m)) {
            return Some((ServiceType::Frontend, None));
        }
        if DART_BACKEND_MARKERS.iter().any(|m| lower.contains(m)) {
            return Some((ServiceType::Backend, Some(8080)));
        }
    }
    Some((ServiceType::Frontend, None))
}

/// Extract a port number from a command string.
/// Matches `--port NNNN`, `-p NNNN`, or `:NNNN` in URLs.
pub fn extract_port(cmd: &str) -> Option<u16> {
    for pat in &["--port ", "-p "] {
        if let Some(pos) = cmd.find(pat) {
            let rest = &cmd[pos + pat.len()..];
            let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(port) = port_str.parse() {
                return Some(port);
            }
        }
    }
    // Match :NNNN in URLs
    if let Some(pos) = cmd.rfind(':') {
        let rest = &cmd[pos + 1..];
        let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if port_str.len() >= 4
            && let Ok(port) = port_str.parse()
        {
            return Some(port);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_port_flag() {
        assert_eq!(extract_port("uvicorn app:main --port 8080"), Some(8080));
        assert_eq!(extract_port("flask run -p 5000"), Some(5000));
    }

    #[test]
    fn test_extract_port_url() {
        assert_eq!(extract_port("http://localhost:3000"), Some(3000));
        assert_eq!(extract_port("no port here"), None);
    }

    #[test]
    fn test_detect_frontend_command() {
        let dir = PathBuf::from(".");
        let (stype, port) = detect_service_info(&dir, "npm run dev");
        assert_eq!(stype, ServiceType::Frontend);
        assert_eq!(port, Some(3000));
    }

    #[test]
    fn test_detect_backend_command() {
        let dir = PathBuf::from(".");
        let (stype, port) = detect_service_info(&dir, "uvicorn app:main --port 9000");
        assert_eq!(stype, ServiceType::Backend);
        assert_eq!(port, Some(9000));
    }

    #[test]
    fn test_detect_python_command() {
        let dir = PathBuf::from(".");
        let (stype, port) = detect_service_info(&dir, "python main.py");
        assert_eq!(stype, ServiceType::Backend);
        assert_eq!(port, Some(8000));
    }

    #[test]
    fn test_detect_worker_command() {
        let dir = PathBuf::from(".");
        let (stype, _) = detect_service_info(&dir, "celery -A tasks worker");
        assert_eq!(stype, ServiceType::Worker);
    }

    #[test]
    fn test_detect_unknown_command() {
        let dir = PathBuf::from(".");
        let (stype, _) = detect_service_info(&dir, "some-random-binary");
        assert_eq!(stype, ServiceType::Unknown);
    }
}
