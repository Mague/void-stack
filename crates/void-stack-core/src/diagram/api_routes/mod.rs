//! API route detection and diagram generation.
//!
//! Supports: Python (FastAPI/Flask), Node.js (Express), gRPC, Swagger/OpenAPI.

mod grpc;
mod node;
mod python;
mod swagger;

use std::path::Path;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Detected API route.
pub struct Route {
    pub method: String,
    pub path: String,
    pub handler: String,
    /// Tag/group from Swagger/OpenAPI docs
    pub tag: Option<String>,
    /// Summary from Swagger/OpenAPI docs
    pub summary: Option<String>,
    /// Whether this is an internal API route
    pub internal: bool,
}

impl Route {
    fn new(method: &str, path: String, handler: String) -> Self {
        let internal = path.to_lowercase().contains("/internal");
        Self {
            method: method.to_string(),
            path,
            handler,
            tag: None,
            summary: None,
            internal,
        }
    }
}

/// Color emoji for HTTP method in diagrams.
fn route_color(method: &str) -> &'static str {
    match method {
        "GET" => "🟢",
        "POST" => "🟡",
        "PUT" => "🟠",
        "PATCH" => "🟠",
        "DELETE" => "🔴",
        "WS" => "🔵",
        "RPC" => "🟣",
        "STREAM" => "🟣",
        _ => "⚪",
    }
}

/// Result of scanning for API routes.
pub struct ApiRouteScanResult {
    pub diagram: String,
    /// Services that were scanned but no routes found (with reason).
    pub skipped: Vec<(String, String)>,
}

/// Generate a Mermaid diagram of API routes found in the project.
pub fn generate(project: &Project) -> String {
    scan(project).diagram
}

/// Scan and return raw route data per service (for use by multiple renderers).
pub fn scan_raw(project: &Project) -> Vec<(String, Vec<Route>)> {
    let mut all_routes = Vec::new();
    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);
        let routes = scan_routes(dir_path);
        if !routes.is_empty() {
            all_routes.push((svc.name.clone(), routes));
        }
    }
    all_routes
}

/// Scan for API routes with detailed results.
pub fn scan(project: &Project) -> ApiRouteScanResult {
    let mut all_routes: Vec<(String, Vec<Route>)> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();

    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);

        let has_py = has_files(
            dir_path,
            &["main.py", "app.py", "server.py", "routes.py", "api.py"],
        );
        let has_js = has_files(
            dir_path,
            &[
                "index.js",
                "index.ts",
                "app.js",
                "app.ts",
                "server.js",
                "server.ts",
            ],
        );
        let has_router_dirs = has_subdir_ci(dir_path, &["routers", "routes", "api", "endpoints"])
            || has_subdir_ci(
                &dir_path.join("src"),
                &["routers", "routes", "api", "endpoints"],
            );

        let routes = scan_routes(dir_path);
        if !routes.is_empty() {
            all_routes.push((svc.name.clone(), routes));
        } else if has_py || has_js || has_router_dirs {
            let total_loc = estimate_loc(dir_path);
            if total_loc > 1000 {
                skipped.push((svc.name.clone(),
                    format!("archivos detectados pero sin rutas parseables ({} LOC — posible codigo demasiado complejo)", total_loc)));
            } else {
                skipped.push((
                    svc.name.clone(),
                    "archivos detectados pero sin decoradores de rutas encontrados".to_string(),
                ));
            }
        }
    }

    if all_routes.is_empty() {
        return ApiRouteScanResult {
            diagram: String::new(),
            skipped,
        };
    }

    let mut lines = vec!["```mermaid".to_string(), "graph LR".to_string()];

    for (svc_name, routes) in &all_routes {
        let svc_id = sanitize_id(svc_name);

        let public_routes: Vec<&Route> = routes.iter().filter(|r| !r.internal).collect();
        let internal_routes: Vec<&Route> = routes.iter().filter(|r| r.internal).collect();

        if !public_routes.is_empty() {
            lines.push(format!("    subgraph {} [\"{}\"]", svc_id, svc_name));
            for (i, route) in public_routes.iter().enumerate() {
                let route_id = format!("{}_{}", svc_id, i);
                let color = route_color(&route.method);
                let label = if let Some(ref summary) = route.summary {
                    format!("{} {} {}\\n{}", color, route.method, route.path, summary)
                } else {
                    format!(
                        "{} {} {}\\n{}",
                        color, route.method, route.path, route.handler
                    )
                };
                lines.push(format!("        {}[\"{}\"]", route_id, label));
            }
            lines.push("    end".to_string());
        }

        if !internal_routes.is_empty() {
            let int_id = format!("{}_internal", svc_id);
            lines.push(format!(
                "    subgraph {} [\"{} — Internal API\"]",
                int_id, svc_name
            ));
            for (i, route) in internal_routes.iter().enumerate() {
                let route_id = format!("{}_int_{}", svc_id, i);
                let color = route_color(&route.method);
                let label = if let Some(ref summary) = route.summary {
                    format!("{} {} {}\\n{}", color, route.method, route.path, summary)
                } else {
                    format!(
                        "{} {} {}\\n{}",
                        color, route.method, route.path, route.handler
                    )
                };
                lines.push(format!("        {}[\"{}\"]", route_id, label));
            }
            lines.push("    end".to_string());
            if !public_routes.is_empty() {
                lines.push(format!("    {} -.->|internal| {}", svc_id, int_id));
            }
        }
    }

    lines.push("```".to_string());
    ApiRouteScanResult {
        diagram: lines.join("\n"),
        skipped,
    }
}

fn scan_routes(dir: &Path) -> Vec<Route> {
    let mut routes = Vec::new();
    python::scan_python_routes(dir, &mut routes);
    node::scan_node_routes(dir, &mut routes);
    grpc::scan_grpc_services(dir, &mut routes);

    let swagger_docs = swagger::scan_swagger_docs(dir);
    if !swagger_docs.is_empty() {
        swagger::enrich_routes_with_swagger(&mut routes, &swagger_docs);
    }

    routes
}

// ── Shared helpers ──────────────────────────────────────────────

fn extract_string_arg(s: &str) -> Option<String> {
    let trimmed = s.trim();
    let quote = if trimmed.starts_with('"') {
        '"'
    } else if trimmed.starts_with('\'') {
        '\''
    } else if trimmed.starts_with('`') {
        '`'
    } else {
        return None;
    };

    let rest = &trimmed[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn has_files(dir: &Path, names: &[&str]) -> bool {
    names.iter().any(|n| dir.join(n).exists())
}

fn estimate_loc(dir: &Path) -> usize {
    let mut total = 0;
    let exts = ["py", "js", "ts", "jsx", "tsx"];
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if exts.contains(&ext)
                    && let Ok(content) = std::fs::read_to_string(&path)
                {
                    total += content.lines().count();
                }
            }
        }
    }
    total
}

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn has_subdir_ci(dir: &Path, names: &[&str]) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            let dirname = entry.file_name().to_string_lossy().to_lowercase();
            if names.iter().any(|n| *n == dirname) {
                return true;
            }
        }
    }
    false
}

pub(super) fn find_subdirs_ci(dir: &Path, names: &[&str]) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return result,
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            let dirname = entry.file_name().to_string_lossy().to_lowercase();
            if names.iter().any(|n| *n == dirname) {
                result.push(entry.path());
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_color() {
        assert_eq!(route_color("GET"), "🟢");
        assert_eq!(route_color("POST"), "🟡");
        assert_eq!(route_color("PUT"), "🟠");
        assert_eq!(route_color("PATCH"), "🟠");
        assert_eq!(route_color("DELETE"), "🔴");
        assert_eq!(route_color("WS"), "🔵");
        assert_eq!(route_color("RPC"), "🟣");
        assert_eq!(route_color("STREAM"), "🟣");
        assert_eq!(route_color("OPTIONS"), "⚪");
    }

    #[test]
    fn test_route_new() {
        let route = Route::new("GET", "/api/users".into(), "list".into());
        assert_eq!(route.method, "GET");
        assert_eq!(route.path, "/api/users");
        assert!(!route.internal);
        assert!(route.tag.is_none());
        assert!(route.summary.is_none());
    }

    #[test]
    fn test_route_internal_detection() {
        let route = Route::new("GET", "/internal/health".into(), "h".into());
        assert!(route.internal);

        let route2 = Route::new("GET", "/api/public".into(), "p".into());
        assert!(!route2.internal);
    }

    #[test]
    fn test_extract_string_arg() {
        assert_eq!(
            extract_string_arg(r#""/users""#),
            Some("/users".to_string())
        );
        assert_eq!(extract_string_arg("'/items'"), Some("/items".to_string()));
        assert_eq!(extract_string_arg("`/ws`"), Some("/ws".to_string()));
        assert_eq!(extract_string_arg("noQuotes"), None);
    }

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("my-service"), "my_service");
        assert_eq!(sanitize_id("api.v1"), "api_v1");
        assert_eq!(sanitize_id("hello_world"), "hello_world");
    }

    #[test]
    fn test_has_subdir_ci() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Routes")).unwrap();

        assert!(has_subdir_ci(dir.path(), &["routes"]));
        assert!(!has_subdir_ci(dir.path(), &["controllers"]));
    }

    #[test]
    fn test_find_subdirs_ci() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("API")).unwrap();
        std::fs::create_dir_all(dir.path().join("other")).unwrap();

        let found = find_subdirs_ci(dir.path(), &["api"]);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_estimate_loc() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.py"), "line1\nline2\nline3\n").unwrap();
        std::fs::write(dir.path().join("other.txt"), "not counted\n").unwrap();

        let loc = estimate_loc(dir.path());
        assert_eq!(loc, 3);
    }

    #[test]
    fn test_has_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.py"), "").unwrap();

        assert!(has_files(dir.path(), &["main.py", "app.py"]));
        assert!(!has_files(dir.path(), &["server.js"]));
    }
}
