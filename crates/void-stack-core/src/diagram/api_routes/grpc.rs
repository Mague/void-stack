//! gRPC / Protobuf service detection.

use std::path::Path;

use super::Route;

pub(super) fn scan_grpc_services(dir: &Path, routes: &mut Vec<Route>) {
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

fn parse_grpc_services(content: &str, routes: &mut Vec<Route>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut current_service = String::new();

    while i < lines.len() {
        let trimmed = lines[i].trim();

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
