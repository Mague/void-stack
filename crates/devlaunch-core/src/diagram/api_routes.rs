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

/// Generate a Mermaid diagram of API routes found in the project.
pub fn generate(project: &Project) -> String {
    let mut all_routes: Vec<(String, Vec<Route>)> = Vec::new();

    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);
        let routes = scan_routes(dir_path);
        if !routes.is_empty() {
            all_routes.push((svc.name.clone(), routes));
        }
    }

    if all_routes.is_empty() {
        return String::new();
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
    lines.join("\n")
}

fn scan_routes(dir: &Path) -> Vec<Route> {
    let mut routes = Vec::new();

    // Scan Python files for FastAPI/Flask routes
    scan_python_routes(dir, &mut routes);

    // Scan JS/TS files for Express routes
    scan_node_routes(dir, &mut routes);

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

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
