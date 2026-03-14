//! Node.js route detection (Express, Fastify, Koa).

use std::path::Path;

use super::{Route, extract_string_arg, find_subdirs_ci};

pub(super) fn scan_node_routes(dir: &Path, routes: &mut Vec<Route>) {
    let js_files = [
        "index.js",
        "index.ts",
        "app.js",
        "app.ts",
        "server.js",
        "server.ts",
        "routes.js",
        "routes.ts",
    ];
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
        let search_dir = if base.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(base)
        };
        for sub_path in find_subdirs_ci(&search_dir, &route_dir_names) {
            if let Ok(entries) = std::fs::read_dir(&sub_path) {
                for entry in entries.flatten() {
                    let ext = entry
                        .path()
                        .extension()
                        .map(|e| e.to_string_lossy().to_string());
                    if matches!(ext.as_deref(), Some("js") | Some("ts"))
                        && let Ok(content) = std::fs::read_to_string(entry.path())
                    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_express_routes() {
        assert!(parse_express_route(r#"app.get("/users", handler)"#).is_some());
        assert!(parse_express_route(r#"router.post("/items", ctrl)"#).is_some());
        assert!(parse_express_route(r#"app.put("/users/:id", update)"#).is_some());
        assert!(parse_express_route(r#"app.delete("/users/:id", del)"#).is_some());
        assert!(parse_express_route(r#"app.patch("/users/:id", patch)"#).is_some());
    }

    #[test]
    fn test_express_route_details() {
        let route = parse_express_route(r#"app.get("/api/v1/products", listProducts)"#).unwrap();
        assert_eq!(route.method, "GET");
        assert_eq!(route.path, "/api/v1/products");
    }

    #[test]
    fn test_express_skips_require_import() {
        assert!(parse_express_route(r#"const x = require("express").get("x")"#).is_none());
        assert!(parse_express_route(r#"import { get("/api") } from "lib""#).is_none());
    }

    #[test]
    fn test_non_route_ignored() {
        assert!(parse_express_route("const app = express()").is_none());
        assert!(parse_express_route("function getUser() {}").is_none());
        assert!(parse_express_route("let x = 42;").is_none());
    }

    #[test]
    fn test_scan_node_routes_from_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.ts"),
            r#"
import express from 'express';
const app = express();

app.get("/health", (req, res) => res.json({ok: true}));
app.post("/users", createUser);
"#,
        )
        .unwrap();

        let mut routes = Vec::new();
        scan_node_routes(dir.path(), &mut routes);

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/health");
    }

    #[test]
    fn test_scan_node_routes_subdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("routes")).unwrap();
        std::fs::write(
            dir.path().join("routes/items.ts"),
            r#"
router.get("/items", list);
router.post("/items", create);
"#,
        )
        .unwrap();

        let mut routes = Vec::new();
        scan_node_routes(dir.path(), &mut routes);

        assert_eq!(routes.len(), 2);
    }
}
