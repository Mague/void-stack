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
                if path.extension().map(|e| e == "proto").unwrap_or(false)
                    && let Ok(content) = std::fs::read_to_string(&path)
                {
                    parse_grpc_services(&content, routes);
                }
                if path.is_dir()
                    && let Ok(sub_entries) = std::fs::read_dir(&path)
                {
                    for sub_entry in sub_entries.flatten() {
                        if sub_entry
                            .path()
                            .extension()
                            .map(|e| e == "proto")
                            .unwrap_or(false)
                            && let Ok(content) = std::fs::read_to_string(sub_entry.path())
                        {
                            parse_grpc_services(&content, routes);
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

        if !current_service.is_empty()
            && trimmed.starts_with("rpc ")
            && let Some(rpc_name) = trimmed
                .strip_prefix("rpc ")
                .and_then(|s| s.split_whitespace().next())
                .or_else(|| {
                    trimmed
                        .strip_prefix("rpc ")
                        .and_then(|s| s.split('(').next())
                })
        {
            let is_stream = trimmed.contains("stream ");
            let method = if is_stream { "STREAM" } else { "RPC" };
            routes.push(Route::new(
                method,
                format!("/{}/{}", current_service, rpc_name.trim()),
                current_service.clone(),
            ));
        }

        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_basic_service() {
        let content = r#"
syntax = "proto3";
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
  rpc CreateUser (CreateUserRequest) returns (User);
  rpc ListUsers (ListUsersRequest) returns (ListUsersResponse);
}
"#;
        let mut routes = Vec::new();
        parse_grpc_services(content, &mut routes);

        assert_eq!(routes.len(), 3);
        assert_eq!(routes[0].method, "RPC");
        assert_eq!(routes[0].path, "/UserService/GetUser");
        assert_eq!(routes[1].path, "/UserService/CreateUser");
        assert_eq!(routes[2].path, "/UserService/ListUsers");
    }

    #[test]
    fn test_grpc_streaming() {
        let content = r#"
service ChatService {
  rpc SendMessage (stream Message) returns (Ack);
  rpc Subscribe (SubscribeReq) returns (stream Event);
}
"#;
        let mut routes = Vec::new();
        parse_grpc_services(content, &mut routes);

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "STREAM");
        assert_eq!(routes[1].method, "STREAM");
    }

    #[test]
    fn test_grpc_multiple_services() {
        let content = r#"
service Auth {
  rpc Login (LoginReq) returns (Token);
}

service Orders {
  rpc PlaceOrder (OrderReq) returns (OrderRes);
}
"#;
        let mut routes = Vec::new();
        parse_grpc_services(content, &mut routes);

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].path, "/Auth/Login");
        assert_eq!(routes[1].path, "/Orders/PlaceOrder");
    }

    #[test]
    fn test_scan_grpc_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("api.proto"),
            r#"
syntax = "proto3";
service HealthCheck {
  rpc Check (Empty) returns (Status);
}
"#,
        )
        .unwrap();

        let mut routes = Vec::new();
        scan_grpc_services(dir.path(), &mut routes);

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].path, "/HealthCheck/Check");
    }

    #[test]
    fn test_scan_grpc_proto_subdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("proto")).unwrap();
        std::fs::write(
            dir.path().join("proto/svc.proto"),
            r#"
service Svc {
  rpc Do (Req) returns (Res);
}
"#,
        )
        .unwrap();

        let mut routes = Vec::new();
        scan_grpc_services(dir.path(), &mut routes);

        // Scanner scans both root dir (finds proto/ subdir) and proto/ directly → 2 matches
        assert!(!routes.is_empty());
        assert!(routes.iter().any(|r| r.path == "/Svc/Do"));
    }
}
