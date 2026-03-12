//! Node.js route detection (Express, Fastify, Koa).

use std::path::Path;

use super::{Route, extract_string_arg, find_subdirs_ci};

pub(super) fn scan_node_routes(dir: &Path, routes: &mut Vec<Route>) {
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
            let before = &line[..pos];
            if before.contains("require") || before.contains("import") {
                continue;
            }
            return Some(Route::new(method, path, "handler".to_string()));
        }
    }

    None
}
