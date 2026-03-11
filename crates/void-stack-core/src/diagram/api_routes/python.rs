//! Python route detection (FastAPI, Flask, Django).

use std::path::Path;

use super::{Route, extract_string_arg, find_subdirs_ci};

pub(super) fn scan_python_routes(dir: &Path, routes: &mut Vec<Route>) {
    let py_files = ["main.py", "app.py", "server.py", "routes.py", "views.py", "api.py"];
    for filename in &py_files {
        let filepath = dir.join(filename);
        let content = match std::fs::read_to_string(&filepath) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for line in content.lines() {
            let trimmed = line.trim();
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
