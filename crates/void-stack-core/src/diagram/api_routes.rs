//! API route detection and diagram generation.

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

        // Check if there are backend files to scan
        let has_py = has_files(dir_path, &["main.py", "app.py", "server.py", "routes.py", "api.py"]);
        let has_js = has_files(dir_path, &["index.js", "index.ts", "app.js", "app.ts", "server.js", "server.ts"]);
        let has_router_dirs = has_subdir_ci(dir_path, &["routers", "routes", "api", "endpoints"])
            || has_subdir_ci(&dir_path.join("src"), &["routers", "routes", "api", "endpoints"]);

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

        // Separate public and internal routes
        let public_routes: Vec<&Route> = routes.iter().filter(|r| !r.internal).collect();
        let internal_routes: Vec<&Route> = routes.iter().filter(|r| r.internal).collect();

        // Public routes subgraph
        if !public_routes.is_empty() {
            lines.push(format!("    subgraph {} [\"{}\"]", svc_id, svc_name));
            for (i, route) in public_routes.iter().enumerate() {
                let route_id = format!("{}_{}", svc_id, i);
                let color = route_color(&route.method);
                let label = if let Some(ref summary) = route.summary {
                    format!("{} {} {}\\n{}", color, route.method, route.path, summary)
                } else {
                    format!("{} {} {}\\n{}", color, route.method, route.path, route.handler)
                };
                lines.push(format!("        {}[\"{}\"]", route_id, label));
            }
            lines.push("    end".to_string());
        }

        // Internal routes subgraph (if any)
        if !internal_routes.is_empty() {
            let int_id = format!("{}_internal", svc_id);
            lines.push(format!("    subgraph {} [\"{} — Internal API\"]", int_id, svc_name));
            for (i, route) in internal_routes.iter().enumerate() {
                let route_id = format!("{}_int_{}", svc_id, i);
                let color = route_color(&route.method);
                let label = if let Some(ref summary) = route.summary {
                    format!("{} {} {}\\n{}", color, route.method, route.path, summary)
                } else {
                    format!("{} {} {}\\n{}", color, route.method, route.path, route.handler)
                };
                lines.push(format!("        {}[\"{}\"]", route_id, label));
            }
            lines.push("    end".to_string());
            // Connect public to internal
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

    // Enrich with Swagger/OpenAPI docs if available
    let swagger_docs = scan_swagger_docs(dir);
    if !swagger_docs.is_empty() {
        enrich_routes_with_swagger(&mut routes, &swagger_docs);
    }

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

    // Also scan subdirectories (routers/, routes/, etc.) — case-insensitive
    let route_dir_names = ["routers", "routes", "api", "endpoints"];
    for base in &["", "src"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        for sub_path in find_subdirs_ci(&search_dir, &route_dir_names) {
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
            let path = extract_string_arg(rest)?;
            let handler = method.to_lowercase();
            return Some(Route::new(method, path, handler));
        }
    }

    // Flask: @app.route("/path", methods=["GET", "POST"])
    if let Some(pos) = line.find("route(") {
        let rest = &line[pos + 6..];
        let path = extract_string_arg(rest)?;
        return Some(Route::new("GET", path, "route".to_string()));
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

    // Check routes/ subdirectory — case-insensitive
    let route_dir_names = ["routes", "api", "routers"];
    for base in &["", "src"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        for sub_path in find_subdirs_ci(&search_dir, &route_dir_names) {
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
            return Some(Route::new(method, path, "handler".to_string()));
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
                routes.push(Route::new(
                    method,
                    format!("/{}/{}", current_service, rpc_name.trim()),
                    current_service.clone(),
                ));
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

/// Check if any subdirectory of `dir` matches one of `names` (case-insensitive).
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

/// Find subdirectories of `dir` that match any of `names` (case-insensitive).
fn find_subdirs_ci(dir: &Path, names: &[&str]) -> Vec<std::path::PathBuf> {
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

// ─── Swagger / OpenAPI ──────────────────────────────────────────────

/// Parsed Swagger/OpenAPI route documentation.
struct SwaggerRoute {
    method: String,
    path: String,
    summary: Option<String>,
    tag: Option<String>,
}

/// Scan for Swagger/OpenAPI documentation files (YAML/JSON).
fn scan_swagger_docs(dir: &Path) -> Vec<SwaggerRoute> {
    let mut docs = Vec::new();

    // Common locations for Swagger/OpenAPI docs (case-insensitive search)
    let doc_dir_names = ["docs", "swagger", "openapi", "api-docs", "apidocs"];
    let doc_file_names = ["swagger.json", "swagger.yaml", "swagger.yml",
        "openapi.json", "openapi.yaml", "openapi.yml"];

    // Check root-level doc files
    for name in &doc_file_names {
        let path = dir.join(name);
        if path.exists() {
            parse_swagger_file(&path, &mut docs);
        }
    }

    // Check doc directories (case-insensitive), including under src/
    for base in &["", "src"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        for sub_path in find_subdirs_ci(&search_dir, &doc_dir_names) {
            scan_swagger_dir(&sub_path, &mut docs);
        }
    }

    docs
}

/// Recursively scan a directory for Swagger YAML/JSON files.
fn scan_swagger_dir(dir: &Path, docs: &mut Vec<SwaggerRoute>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_swagger_dir(&path, docs);
            } else {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if matches!(ext, "yml" | "yaml" | "json") {
                    parse_swagger_file(&path, docs);
                }
            }
        }
    }
}

/// Parse a single Swagger/OpenAPI YAML or JSON file for route definitions.
/// Handles both full OpenAPI specs and individual swagger-jsdoc YAML fragments.
fn parse_swagger_file(path: &Path, docs: &mut Vec<SwaggerRoute>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Detect if this is a YAML fragment (swagger-jsdoc style) with path definitions
    // Format:
    //   /api/users:
    //     get:
    //       summary: List users
    //       tags:
    //         - Users
    parse_swagger_yaml_routes(&content, docs);
}

/// Parse Swagger YAML content for route definitions.
/// Handles both full OpenAPI specs and swagger-jsdoc fragments.
fn parse_swagger_yaml_routes(content: &str, docs: &mut Vec<SwaggerRoute>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut current_path: Option<String> = None;
    let mut current_method: Option<String> = None;
    let mut current_summary: Option<String> = None;
    let mut current_tag: Option<String> = None;
    let mut in_tags = false;
    let mut path_indent = 0;
    let mut method_indent = 0;

    let http_methods = ["get", "post", "put", "delete", "patch", "options", "head"];

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }

        // Detect path definition: /api/users: or '/api/users':
        if trimmed.ends_with(':') || trimmed.contains("':") || trimmed.contains("\":") {
            let clean = trimmed.trim_end_matches(':').trim().trim_matches('\'').trim_matches('"');
            if clean.starts_with('/') {
                // Save previous method if any
                if let (Some(p), Some(m)) = (&current_path, &current_method) {
                    docs.push(SwaggerRoute {
                        method: m.to_uppercase(),
                        path: p.clone(),
                        summary: current_summary.take(),
                        tag: current_tag.take(),
                    });
                }
                current_path = Some(clean.to_string());
                current_method = None;
                current_summary = None;
                current_tag = None;
                in_tags = false;
                path_indent = indent;
                i += 1;
                continue;
            }
        }

        // Inside a path definition — look for HTTP methods
        if current_path.is_some() && indent > path_indent {
            let key = trimmed.trim_end_matches(':').trim().to_lowercase();
            if http_methods.contains(&key.as_str()) {
                // Save previous method if any
                if let (Some(p), Some(m)) = (&current_path, &current_method) {
                    docs.push(SwaggerRoute {
                        method: m.to_uppercase(),
                        path: p.clone(),
                        summary: current_summary.take(),
                        tag: current_tag.take(),
                    });
                }
                current_method = Some(key);
                current_summary = None;
                current_tag = None;
                in_tags = false;
                method_indent = indent;
                i += 1;
                continue;
            }

            // Inside a method — look for summary and tags
            if current_method.is_some() && indent > method_indent {
                if trimmed.starts_with("summary:") {
                    current_summary = Some(
                        trimmed.strip_prefix("summary:").unwrap_or("").trim()
                            .trim_matches('\'').trim_matches('"').to_string()
                    );
                    in_tags = false;
                } else if trimmed.starts_with("tags:") {
                    in_tags = true;
                } else if in_tags && trimmed.starts_with("- ") {
                    current_tag = Some(
                        trimmed.strip_prefix("- ").unwrap_or("").trim()
                            .trim_matches('\'').trim_matches('"').to_string()
                    );
                    in_tags = false;
                } else if !trimmed.starts_with("- ") {
                    in_tags = false;
                }
            }
        } else if current_path.is_some() && indent <= path_indent {
            // Exited path block — save last method
            if let (Some(p), Some(m)) = (&current_path, &current_method) {
                docs.push(SwaggerRoute {
                    method: m.to_uppercase(),
                    path: p.clone(),
                    summary: current_summary.take(),
                    tag: current_tag.take(),
                });
            }
            current_path = None;
            current_method = None;
            in_tags = false;
            // Don't increment i — re-process this line
            continue;
        }

        i += 1;
    }

    // Save last route
    if let (Some(p), Some(m)) = (&current_path, &current_method) {
        docs.push(SwaggerRoute {
            method: m.to_uppercase(),
            path: p.clone(),
            summary: current_summary,
            tag: current_tag,
        });
    }
}

/// Enrich detected routes with Swagger documentation (summary, tag).
/// Also adds routes found only in Swagger docs but not detected by code scanning.
fn enrich_routes_with_swagger(routes: &mut Vec<Route>, swagger: &[SwaggerRoute]) {
    for sdoc in swagger {
        // Try to find matching route by path and method
        let found = routes.iter_mut().find(|r| {
            r.method == sdoc.method && normalize_path(&r.path) == normalize_path(&sdoc.path)
        });

        if let Some(route) = found {
            // Enrich existing route
            if route.summary.is_none() {
                route.summary = sdoc.summary.clone();
            }
            if route.tag.is_none() {
                route.tag = sdoc.tag.clone();
            }
        } else {
            // Route only in Swagger — add it
            let mut route = Route::new(&sdoc.method, sdoc.path.clone(), "swagger".to_string());
            route.summary = sdoc.summary.clone();
            route.tag = sdoc.tag.clone();
            routes.push(route);
        }
    }
}

/// Normalize route path for comparison (strip param names, lowercase).
fn normalize_path(path: &str) -> String {
    let mut result = String::new();
    let mut in_param = false;
    for ch in path.chars() {
        if ch == '{' || ch == ':' {
            in_param = true;
            result.push('{');
        } else if in_param && (ch == '}' || ch == '/') {
            in_param = false;
            result.push('}');
            if ch == '/' {
                result.push('/');
            }
        } else if !in_param {
            result.push(ch);
        }
    }
    result.to_lowercase()
}
