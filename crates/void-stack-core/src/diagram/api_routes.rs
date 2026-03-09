//! API route detection and diagram generation.

use std::path::Path;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// Detected API route.
struct Route {
    method: String,
    path: String,
    handler: String,
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

/// Scan for API routes with detailed results.
pub fn scan(project: &Project) -> ApiRouteScanResult {
    let mut all_routes: Vec<(String, Vec<Route>)> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();

    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);

        // Check if there are backend files to scan
        let has_py = has_files(dir_path, &["main.py", "app.py", "server.py", "routes.py", "api.py"]);
        let has_js = has_files(dir_path, &["index.js", "index.ts", "app.js", "app.ts", "server.js", "server.ts"]);
        let has_router_dirs = ["routers", "routes", "api", "endpoints"]
            .iter().any(|d| dir_path.join(d).is_dir());

        let routes = scan_routes(dir_path);
        if !routes.is_empty() {
            all_routes.push((svc.name.clone(), routes));
        } else if has_py || has_js || has_router_dirs {
            // There are source files but no routes detected
            let total_loc = estimate_loc(dir_path);
            if total_loc > 1000 {
                skipped.push((svc.name.clone(),
                    format!("archivos detectados pero sin rutas parseables ({} LOC — posible codigo demasiado complejo)", total_loc)));
            } else {
                skipped.push((svc.name.clone(),
                    "archivos detectados pero sin decoradores de rutas encontrados".to_string()));
            }
        }
    }

    if all_routes.is_empty() {
        return ApiRouteScanResult {
            diagram: String::new(),
            skipped,
        };
    }

    let mut lines = vec![
        "```mermaid".to_string(),
        "graph LR".to_string(),
    ];

    for (svc_name, routes) in &all_routes {
        let svc_id = sanitize_id(svc_name);
        lines.push(format!("    subgraph {} [\"{}\"]", svc_id, svc_name));

        for (i, route) in routes.iter().enumerate() {
            let route_id = format!("{}_{}", svc_id, i);
            let color = match route.method.as_str() {
                "GET" => "🟢",
                "POST" => "🟡",
                "PUT" => "🔵",
                "DELETE" => "🔴",
                "PATCH" => "🟣",
                "WS" => "⚡",
                "RPC" => "🔷",
                "STREAM" => "🔶",
                _ => "⚪",
            };
            lines.push(format!(
                "        {}[\"{} {} {}\\n{}\"]",
                route_id, color, route.method, route.path, route.handler,
            ));
        }
        lines.push("    end".to_string());
    }

    lines.push("```".to_string());
    ApiRouteScanResult {
        diagram: lines.join("\n"),
        skipped,
    }
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
                if exts.contains(&ext) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        total += content.lines().count();
                    }
                }
            }
        }
    }
    total
}

fn scan_routes(dir: &Path) -> Vec<Route> {
    let mut routes = Vec::new();

    // Scan Python files for FastAPI/Flask routes
    scan_python_routes(dir, &mut routes);

    // Scan JS/TS files for Express routes
    scan_node_routes(dir, &mut routes);

    // Scan .proto files for gRPC service definitions
    scan_grpc_services(dir, &mut routes);

    routes
}

fn scan_python_routes(dir: &Path, routes: &mut Vec<Route>) {
    let py_files = ["main.py", "app.py", "server.py", "routes.py", "views.py", "api.py"];
    for filename in &py_files {
        let filepath = dir.join(filename);
        let content = match std::fs::read_to_string(&filepath) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for line in content.lines() {
            let trimmed = line.trim();

            // FastAPI: @app.get("/path"), @router.post("/path")
            if let Some(route) = parse_python_decorator(trimmed) {
                routes.push(route);
            }
        }
    }

    // Also scan subdirectories (routers/)
    for subdir in &["routers", "routes", "api", "endpoints"] {
        let sub_path = dir.join(subdir);
        if sub_path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&sub_path) {
                for entry in entries.flatten() {
                    if entry.path().extension().map(|e| e == "py").unwrap_or(false) {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            for line in content.lines() {
                                if let Some(route) = parse_python_decorator(line.trim()) {
                                    routes.push(route);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn parse_python_decorator(line: &str) -> Option<Route> {
    // @app.get("/users"), @router.post("/items/{id}"), @app.websocket("/ws")
    let methods = [
        ("get(", "GET"),
        ("post(", "POST"),
        ("put(", "PUT"),
        ("delete(", "DELETE"),
        ("patch(", "PATCH"),
        ("websocket(", "WS"),
    ];

    if !line.starts_with('@') {
        return None;
    }

    for (pattern, method) in &methods {
        if let Some(pos) = line.find(pattern) {
            let rest = &line[pos + pattern.len()..];
            // Extract the path string
            let path = extract_string_arg(rest)?;
            let handler = format!("{}", method.to_lowercase());
            return Some(Route {
                method: method.to_string(),
                path,
                handler,
            });
        }
    }

    // Flask: @app.route("/path", methods=["GET", "POST"])
    if let Some(pos) = line.find("route(") {
        let rest = &line[pos + 6..];
        let path = extract_string_arg(rest)?;
        return Some(Route {
            method: "GET".to_string(),
            path,
            handler: "route".to_string(),
        });
    }

    None
}

fn scan_node_routes(dir: &Path, routes: &mut Vec<Route>) {
    let js_files = ["index.js", "index.ts", "app.js", "app.ts", "server.js", "server.ts",
                    "routes.js", "routes.ts"];
    for filename in &js_files {
        let filepath = dir.join(filename);
        let content = match std::fs::read_to_string(&filepath) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(route) = parse_express_route(trimmed) {
                routes.push(route);
            }
        }
    }

    // Check routes/ subdirectory
    for subdir in &["routes", "api"] {
        let sub_path = dir.join(subdir);
        if sub_path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&sub_path) {
                for entry in entries.flatten() {
                    let ext = entry.path().extension().map(|e| e.to_string_lossy().to_string());
                    if matches!(ext.as_deref(), Some("js") | Some("ts")) {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            for line in content.lines() {
                                if let Some(route) = parse_express_route(line.trim()) {
                                    routes.push(route);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn parse_express_route(line: &str) -> Option<Route> {
    // app.get("/users", handler), router.post("/items", handler)
    let methods = [
        (".get(", "GET"),
        (".post(", "POST"),
        (".put(", "PUT"),
        (".delete(", "DELETE"),
        (".patch(", "PATCH"),
    ];

    for (pattern, method) in &methods {
        if let Some(pos) = line.find(pattern) {
            let rest = &line[pos + pattern.len()..];
            let path = extract_string_arg(rest)?;
            // Don't match non-route patterns like require.get()
            let before = &line[..pos];
            if before.contains("require") || before.contains("import") {
                continue;
            }
            return Some(Route {
                method: method.to_string(),
                path,
                handler: "handler".to_string(),
            });
        }
    }

    None
}

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

// ─── gRPC / Protobuf ────────────────────────────────────────────────

fn scan_grpc_services(dir: &Path, routes: &mut Vec<Route>) {
    // Scan proto/ and protos/ subdirectories and current dir
    let dirs_to_scan: Vec<std::path::PathBuf> = std::iter::once(dir.to_path_buf())
        .chain(["proto", "protos", "lib/proto"].iter().map(|d| dir.join(d)))
        .collect();

    for scan_dir in dirs_to_scan {
        if let Ok(entries) = std::fs::read_dir(&scan_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "proto").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        parse_grpc_services(&content, routes);
                    }
                }
                // Recurse one level into subdirectories
                if path.is_dir() {
                    if let Ok(sub_entries) = std::fs::read_dir(&path) {
                        for sub_entry in sub_entries.flatten() {
                            if sub_entry.path().extension().map(|e| e == "proto").unwrap_or(false) {
                                if let Ok(content) = std::fs::read_to_string(sub_entry.path()) {
                                    parse_grpc_services(&content, routes);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Parse gRPC service definitions from .proto files:
/// ```proto
/// service UserService {
///   rpc GetUser (GetUserRequest) returns (UserResponse);
///   rpc ListUsers (ListUsersRequest) returns (stream UserResponse);
/// }
/// ```
fn parse_grpc_services(content: &str, routes: &mut Vec<Route>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut current_service = String::new();

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // service MyService {
        if trimmed.starts_with("service ") && trimmed.ends_with('{') {
            current_service = trimmed
                .strip_prefix("service ")
                .and_then(|s| s.strip_suffix('{'))
                .unwrap_or("")
                .trim()
                .to_string();
            i += 1;
            continue;
        }

        if trimmed == "}" {
            current_service.clear();
            i += 1;
            continue;
        }

        // rpc MethodName (RequestType) returns (ResponseType);
        if !current_service.is_empty() && trimmed.starts_with("rpc ") {
            if let Some(rpc_name) = trimmed
                .strip_prefix("rpc ")
                .and_then(|s| s.split_whitespace().next())
                .or_else(|| trimmed.strip_prefix("rpc ").and_then(|s| s.split('(').next()))
            {
                let is_stream = trimmed.contains("stream ");
                let method = if is_stream { "STREAM" } else { "RPC" };
                routes.push(Route {
                    method: method.to_string(),
                    path: format!("/{}/{}", current_service, rpc_name.trim()),
                    handler: current_service.clone(),
                });
            }
        }

        i += 1;
    }
}

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
