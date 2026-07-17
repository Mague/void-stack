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

/// Routes grouped per service.
pub(in crate::diagram) type ServiceRoutes = Vec<(String, Vec<Route>)>;
/// (service, reason) pairs for services that looked like APIs but yielded
/// no parseable routes.
pub(in crate::diagram) type SkippedServices = Vec<(String, String)>;

/// Scan every service for routes. Only called by `ir::build_ir` —
/// renderers consume the IR.
pub(in crate::diagram) fn scan_project(project: &Project) -> (ServiceRoutes, SkippedServices) {
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

    (all_routes, skipped)
}

/// Render the Mermaid API-routes diagram. `None` when there is nothing to
/// show. Public/internal routes are split into separate subgraphs.
pub(in crate::diagram) fn render_mermaid(all_routes: &[(String, Vec<Route>)]) -> Option<String> {
    if all_routes.iter().all(|(_, r)| r.is_empty()) {
        return None;
    }

    let mut lines = vec!["```mermaid".to_string(), "graph LR".to_string()];

    for (svc_name, routes) in all_routes {
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
    Some(lines.join("\n"))
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

    // ── Fixture helpers ─────────────────────────────────────────

    fn make_service(name: &str, dir: &Path) -> crate::model::Service {
        crate::model::Service {
            name: name.to_string(),
            command: "run".to_string(),
            target: crate::model::Target::native(),
            working_dir: Some(dir.to_string_lossy().to_string()),
            enabled: true,
            env_vars: Vec::new(),
            depends_on: Vec::new(),
            docker: None,
        }
    }

    fn make_project(path: &Path, services: Vec<crate::model::Service>) -> Project {
        Project {
            name: "fixture".to_string(),
            description: String::new(),
            path: path.to_string_lossy().to_string(),
            project_type: None,
            tags: Vec::new(),
            services,
            hooks: None,
        }
    }

    // ── scan_routes / scan_project ──────────────────────────────

    #[test]
    fn test_scan_routes_combines_python_and_node() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            "@app.get(\"/py\")\ndef py_handler():\n    pass\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("app.js"), "app.post(\"/js\", handler);\n").unwrap();

        let routes = scan_routes(dir.path());
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().any(|r| r.method == "GET" && r.path == "/py"));
        assert!(routes.iter().any(|r| r.method == "POST" && r.path == "/js"));
    }

    #[test]
    fn test_scan_project_finds_fastapi_routes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            r#"
from fastapi import FastAPI
app = FastAPI()

@app.get("/health")
def health():
    return {"ok": True}

@app.post("/users")
def create_user():
    pass
"#,
        )
        .unwrap();

        let project = make_project(dir.path(), vec![make_service("api", dir.path())]);
        let (all_routes, skipped) = scan_project(&project);

        assert!(skipped.is_empty());
        assert_eq!(all_routes.len(), 1);
        let (svc_name, routes) = &all_routes[0];
        assert_eq!(svc_name, "api");
        assert_eq!(routes.len(), 2);
    }

    #[test]
    fn test_scan_project_finds_express_routes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("server.ts"),
            "app.get(\"/items\", list);\napp.delete(\"/items/:id\", remove);\n",
        )
        .unwrap();

        let project = make_project(dir.path(), vec![make_service("web", dir.path())]);
        let (all_routes, skipped) = scan_project(&project);

        assert!(skipped.is_empty());
        assert_eq!(all_routes.len(), 1);
        assert_eq!(all_routes[0].1.len(), 2);
    }

    #[test]
    fn test_scan_project_skips_service_without_parseable_routes() {
        // A main.py without route decorators looks like an API but yields
        // nothing — the service must be reported as skipped.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.py"), "print(\"hello\")\n").unwrap();

        let project = make_project(dir.path(), vec![make_service("cli", dir.path())]);
        let (all_routes, skipped) = scan_project(&project);

        assert!(all_routes.is_empty());
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].0, "cli");
        assert!(skipped[0].1.contains("sin decoradores"));
    }

    #[test]
    fn test_scan_project_skips_large_service_with_loc_hint() {
        // >1000 LOC without routes gets the "too complex" skip reason.
        let dir = tempfile::tempdir().unwrap();
        let big = "x = 1\n".repeat(1500);
        std::fs::write(dir.path().join("app.py"), big).unwrap();

        let project = make_project(dir.path(), vec![make_service("big", dir.path())]);
        let (all_routes, skipped) = scan_project(&project);

        assert!(all_routes.is_empty());
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].1.contains("LOC"));
    }

    #[test]
    fn test_scan_project_ignores_non_api_service() {
        // No API-looking files at all: neither routes nor skipped entries.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "nothing here").unwrap();

        let project = make_project(dir.path(), vec![make_service("misc", dir.path())]);
        let (all_routes, skipped) = scan_project(&project);

        assert!(all_routes.is_empty());
        assert!(skipped.is_empty());
    }

    // ── render_mermaid ──────────────────────────────────────────

    #[test]
    fn test_render_mermaid_none_when_empty() {
        assert!(render_mermaid(&[]).is_none());
        let empty: Vec<(String, Vec<Route>)> = vec![("svc".to_string(), Vec::new())];
        assert!(render_mermaid(&empty).is_none());
    }

    #[test]
    fn test_render_mermaid_public_routes() {
        let routes = vec![(
            "my-api".to_string(),
            vec![
                Route::new("GET", "/users".into(), "list_users".into()),
                Route::new("POST", "/users".into(), "create_user".into()),
            ],
        )];
        let out = render_mermaid(&routes).unwrap();

        assert!(out.starts_with("```mermaid"));
        assert!(out.ends_with("```"));
        assert!(out.contains("graph LR"));
        // Service subgraph uses the sanitized id and original label.
        assert!(out.contains("subgraph my_api [\"my-api\"]"));
        assert!(out.contains("GET /users"));
        assert!(out.contains("POST /users"));
        assert!(out.contains("list_users"));
        // No internal subgraph for public-only routes.
        assert!(!out.contains("Internal API"));
    }

    #[test]
    fn test_render_mermaid_internal_routes_split_and_linked() {
        let routes = vec![(
            "svc".to_string(),
            vec![
                Route::new("GET", "/public".into(), "pub_handler".into()),
                Route::new("POST", "/internal/sync".into(), "sync".into()),
            ],
        )];
        let out = render_mermaid(&routes).unwrap();

        assert!(out.contains("subgraph svc [\"svc\"]"));
        assert!(out.contains("subgraph svc_internal [\"svc — Internal API\"]"));
        // Public subgraph links to the internal one.
        assert!(out.contains("svc -.->|internal| svc_internal"));
    }

    #[test]
    fn test_render_mermaid_internal_only_no_link() {
        let routes = vec![(
            "svc".to_string(),
            vec![Route::new(
                "DELETE",
                "/internal/purge".into(),
                "purge".into(),
            )],
        )];
        let out = render_mermaid(&routes).unwrap();

        assert!(out.contains("Internal API"));
        // Without public routes there is nothing to link from.
        assert!(!out.contains("-.->|internal|"));
    }

    #[test]
    fn test_render_mermaid_prefers_summary_over_handler() {
        let mut route = Route::new("GET", "/orders".into(), "handler_name".into());
        route.summary = Some("List all orders".to_string());
        let routes = vec![("svc".to_string(), vec![route])];
        let out = render_mermaid(&routes).unwrap();

        assert!(out.contains("List all orders"));
        assert!(!out.contains("handler_name"));
    }
}
