//! Python route detection (FastAPI, Flask, Django).

use std::path::Path;

use super::{Route, extract_string_arg, find_subdirs_ci};

pub(super) fn scan_python_routes(dir: &Path, routes: &mut Vec<Route>) {
    let py_files = [
        "main.py",
        "app.py",
        "server.py",
        "routes.py",
        "views.py",
        "api.py",
    ];
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
        let search_dir = if base.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(base)
        };
        for sub_path in find_subdirs_ci(&search_dir, &route_dir_names) {
            if let Ok(entries) = std::fs::read_dir(&sub_path) {
                for entry in entries.flatten() {
                    if entry.path().extension().map(|e| e == "py").unwrap_or(false)
                        && let Ok(content) = std::fs::read_to_string(entry.path())
                    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fastapi_decorators() {
        assert!(parse_python_decorator(r#"@app.get("/users")"#).is_some());
        assert!(parse_python_decorator(r#"@router.post("/items")"#).is_some());
        assert!(parse_python_decorator(r#"@app.put("/users/{id}")"#).is_some());
        assert!(parse_python_decorator(r#"@app.delete("/users/{id}")"#).is_some());
        assert!(parse_python_decorator(r#"@app.patch("/users/{id}")"#).is_some());
        assert!(parse_python_decorator(r#"@app.websocket("/ws")"#).is_some());
    }

    #[test]
    fn test_fastapi_route_details() {
        let route = parse_python_decorator(r#"@app.get("/api/v1/products")"#).unwrap();
        assert_eq!(route.method, "GET");
        assert_eq!(route.path, "/api/v1/products");
    }

    #[test]
    fn test_flask_route() {
        let route = parse_python_decorator(r#"@app.route("/dashboard")"#).unwrap();
        assert_eq!(route.method, "GET");
        assert_eq!(route.path, "/dashboard");
    }

    #[test]
    fn test_non_decorator_ignored() {
        assert!(parse_python_decorator("def get_user():").is_none());
        assert!(parse_python_decorator("# @app.get(\"/test\")").is_none());
        assert!(parse_python_decorator("x = 42").is_none());
    }

    #[test]
    fn test_scan_python_routes_from_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            r#"
from fastapi import FastAPI
app = FastAPI()

@app.get("/health")
def health():
    return {"status": "ok"}

@app.post("/users")
def create_user():
    pass
"#,
        )
        .unwrap();

        let mut routes = Vec::new();
        scan_python_routes(dir.path(), &mut routes);

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/health");
        assert_eq!(routes[1].method, "POST");
        assert_eq!(routes[1].path, "/users");
    }

    #[test]
    fn test_scan_python_routes_subdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("routers")).unwrap();
        std::fs::write(
            dir.path().join("routers/users.py"),
            r#"
@router.get("/users")
def list_users():
    pass

@router.delete("/users/{id}")
def delete_user():
    pass
"#,
        )
        .unwrap();

        let mut routes = Vec::new();
        scan_python_routes(dir.path(), &mut routes);

        assert_eq!(routes.len(), 2);
    }
}
