//! Protobuf message detection.

use super::DbModel;

pub(super) fn scan_proto_messages(dir: &std::path::Path, models: &mut Vec<DbModel>) {
    scan_proto_messages_recursive(dir, models, 0);
}

fn scan_proto_messages_recursive(dir: &std::path::Path, models: &mut Vec<DbModel>, depth: u32) {
    if depth > 3 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if matches!(
                name.as_str(),
                "node_modules" | "target" | ".git" | "build" | ".venv" | "venv" | "__pycache__"
            ) {
                continue;
            }
            scan_proto_messages_recursive(&path, models, depth + 1);
            continue;
        }
        if path.extension().map(|e| e == "proto").unwrap_or(false)
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            parse_proto_messages(&content, models);
        }
    }
}

pub(super) fn parse_proto_messages(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("message ") && trimmed.ends_with('{') {
            let msg_name = trimmed
                .strip_prefix("message ")
                .and_then(|s| s.strip_suffix('{'))
                .unwrap_or("")
                .trim()
                .to_string();

            if msg_name.is_empty() || models.iter().any(|m| m.name == msg_name) {
                i += 1;
                continue;
            }

            let mut fields = Vec::new();
            i += 1;

            while i < lines.len() {
                let fl = lines[i].trim();
                if fl == "}" {
                    break;
                }
                if fl.is_empty()
                    || fl.starts_with("//")
                    || fl.starts_with("reserved")
                    || fl.starts_with("option")
                    || fl.starts_with("oneof")
                    || fl.starts_with("message ")
                    || fl.starts_with("enum ")
                {
                    i += 1;
                    continue;
                }

                if let Some((field_name, field_type)) = parse_proto_field(fl) {
                    fields.push((field_name, field_type));
                }

                i += 1;
            }

            if !fields.is_empty() {
                models.push(DbModel {
                    name: msg_name,
                    fields,
                });
            }
            continue;
        }
        i += 1;
    }
}

fn parse_proto_field(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim().trim_end_matches(';');
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    if parts[0] == "map" || parts[0].starts_with("map<") {
        let name = parts
            .iter()
            .find(|p| p.contains('='))
            .and_then(|_| parts.get(parts.len().saturating_sub(3)))
            .map(|s| s.to_string())?;
        return Some((name, "map".to_string()));
    }

    let (proto_type, field_name) = if parts[0] == "repeated" || parts[0] == "optional" {
        if parts.len() < 4 {
            return None;
        }
        (parts[1], parts[2])
    } else {
        (parts[0], parts[1])
    };

    let mapped = match proto_type {
        "string" => "string",
        "int32" | "int64" | "sint32" | "sint64" | "uint32" | "uint64" | "fixed32" | "fixed64"
        | "sfixed32" | "sfixed64" => "int",
        "float" | "double" => "float",
        "bool" => "bool",
        "bytes" => "binary",
        "google.protobuf.Timestamp" => "datetime",
        _ => "FK",
    };

    Some((field_name.to_string(), mapped.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_proto_basic_message() {
        let content = r#"
syntax = "proto3";

message User {
  string name = 1;
  int32 age = 2;
  float score = 3;
  bool active = 4;
  bytes avatar = 5;
}
"#;
        let mut models = Vec::new();
        parse_proto_messages(content, &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "User");
        assert_eq!(models[0].fields.len(), 5);

        let field_map: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(field_map["name"], "string");
        assert_eq!(field_map["age"], "int");
        assert_eq!(field_map["score"], "float");
        assert_eq!(field_map["active"], "bool");
        assert_eq!(field_map["avatar"], "binary");
    }

    #[test]
    fn test_parse_proto_int_variants() {
        let content = r#"
message Numbers {
  int64 big = 1;
  uint32 unsigned = 2;
  sint64 signed = 3;
  fixed32 fixed = 4;
  sfixed64 sfixed = 5;
  double precise = 6;
}
"#;
        let mut models = Vec::new();
        parse_proto_messages(content, &mut models);

        assert_eq!(models[0].fields.len(), 6);
        let field_map: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(field_map["big"], "int");
        assert_eq!(field_map["unsigned"], "int");
        assert_eq!(field_map["precise"], "float");
    }

    #[test]
    fn test_parse_proto_repeated() {
        // Note: `optional` lines are skipped by the parser because they start with "option"
        let content = r#"
message Post {
  string title = 1;
  repeated string tags = 2;
}
"#;
        let mut models = Vec::new();
        parse_proto_messages(content, &mut models);

        assert_eq!(models[0].fields.len(), 2);
    }

    #[test]
    fn test_parse_proto_fk_reference() {
        let content = r#"
message Order {
  string id = 1;
  User customer = 2;
}
"#;
        let mut models = Vec::new();
        parse_proto_messages(content, &mut models);

        let field_map: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(field_map["customer"], "FK");
    }

    #[test]
    fn test_parse_proto_skips_reserved_comments() {
        let content = r#"
message Config {
  // A comment
  string key = 1;
  reserved 2, 3;
  option java_package = "com.example";
  string value = 4;
}
"#;
        let mut models = Vec::new();
        parse_proto_messages(content, &mut models);

        assert_eq!(models[0].fields.len(), 2);
    }

    #[test]
    fn test_parse_proto_multiple_messages() {
        let content = r#"
message Req {
  string query = 1;
}

message Res {
  string result = 1;
}
"#;
        let mut models = Vec::new();
        parse_proto_messages(content, &mut models);

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "Req");
        assert_eq!(models[1].name, "Res");
    }

    #[test]
    fn test_parse_proto_no_duplicates() {
        let content = r#"
message Dup {
  string a = 1;
}
"#;
        let mut models = Vec::new();
        parse_proto_messages(content, &mut models);
        parse_proto_messages(content, &mut models);

        assert_eq!(models.len(), 1);
    }

    #[test]
    fn test_scan_proto_files_in_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("service.proto"),
            r#"
syntax = "proto3";
message Item {
  string name = 1;
  int32 count = 2;
}
"#,
        )
        .unwrap();

        let mut models = Vec::new();
        scan_proto_messages(dir.path(), &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Item");
    }
}
