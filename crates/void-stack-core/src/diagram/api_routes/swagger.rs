//! Swagger / OpenAPI documentation parsing.

use std::path::Path;

use super::{Route, find_subdirs_ci};

/// Parsed Swagger/OpenAPI route documentation.
pub(super) struct SwaggerRoute {
    pub method: String,
    pub path: String,
    pub summary: Option<String>,
    pub tag: Option<String>,
}

/// Scan for Swagger/OpenAPI documentation files (YAML/JSON).
pub(super) fn scan_swagger_docs(dir: &Path) -> Vec<SwaggerRoute> {
    let mut docs = Vec::new();

    let doc_dir_names = ["docs", "swagger", "openapi", "api-docs", "apidocs"];
    let doc_file_names = ["swagger.json", "swagger.yaml", "swagger.yml",
        "openapi.json", "openapi.yaml", "openapi.yml"];

    for name in &doc_file_names {
        let path = dir.join(name);
        if path.exists() {
            parse_swagger_file(&path, &mut docs);
        }
    }

    for base in &["", "src"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        for sub_path in find_subdirs_ci(&search_dir, &doc_dir_names) {
            scan_swagger_dir(&sub_path, &mut docs);
        }
    }

    docs
}

/// Enrich detected routes with Swagger documentation (summary, tag).
/// Also adds routes found only in Swagger docs but not detected by code scanning.
pub(super) fn enrich_routes_with_swagger(routes: &mut Vec<Route>, swagger: &[SwaggerRoute]) {
    for sdoc in swagger {
        let found = routes.iter_mut().find(|r| {
            r.method == sdoc.method && normalize_path(&r.path) == normalize_path(&sdoc.path)
        });

        if let Some(route) = found {
            if route.summary.is_none() {
                route.summary = sdoc.summary.clone();
            }
            if route.tag.is_none() {
                route.tag = sdoc.tag.clone();
            }
        } else {
            let mut route = Route::new(&sdoc.method, sdoc.path.clone(), "swagger".to_string());
            route.summary = sdoc.summary.clone();
            route.tag = sdoc.tag.clone();
            routes.push(route);
        }
    }
}

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

fn parse_swagger_file(path: &Path, docs: &mut Vec<SwaggerRoute>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    parse_swagger_yaml_routes(&content, docs);
}

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

        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }

        if trimmed.ends_with(':') || trimmed.contains("':") || trimmed.contains("\":") {
            let clean = trimmed.trim_end_matches(':').trim().trim_matches('\'').trim_matches('"');
            if clean.starts_with('/') {
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

        if current_path.is_some() && indent > path_indent {
            let key = trimmed.trim_end_matches(':').trim().to_lowercase();
            if http_methods.contains(&key.as_str()) {
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
            continue;
        }

        i += 1;
    }

    if let (Some(p), Some(m)) = (&current_path, &current_method) {
        docs.push(SwaggerRoute {
            method: m.to_uppercase(),
            path: p.clone(),
            summary: current_summary,
            tag: current_tag,
        });
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
