//! GORM (Go) model detection.

use super::DbModel;

pub(super) fn scan_gorm_models(dir: &std::path::Path, models: &mut Vec<DbModel>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if entry.path().extension().map(|e| e == "go").unwrap_or(false)
            && let Ok(content) = std::fs::read_to_string(entry.path())
        {
            parse_gorm_structs(&content, models);
        }
    }
}

/// Parse Go structs that embed gorm.Model or have `gorm:"..."` tags.
fn parse_gorm_structs(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("type ") && trimmed.contains("struct") && trimmed.ends_with('{') {
            let struct_name = trimmed
                .strip_prefix("type ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("")
                .to_string();

            i += 1;
            let mut fields = Vec::new();
            let mut is_gorm_model = false;

            while i < lines.len() {
                let fl = lines[i].trim();
                if fl == "}" {
                    break;
                }

                if fl == "gorm.Model" || fl.starts_with("gorm.Model") {
                    is_gorm_model = true;
                    i += 1;
                    continue;
                }

                if fl.contains("gorm:\"") || fl.contains("`gorm:") {
                    is_gorm_model = true;
                }

                if let Some((name, go_type)) = parse_go_struct_field(fl) {
                    fields.push((name, go_type));
                }

                i += 1;
            }

            if is_gorm_model
                && !fields.is_empty()
                && !struct_name.is_empty()
                && !models.iter().any(|m| m.name == struct_name)
            {
                models.push(DbModel {
                    name: struct_name,
                    fields,
                });
            }
            continue;
        }
        i += 1;
    }
}

fn parse_go_struct_field(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let name = parts[0];
    if name.chars().next()?.is_lowercase() && name != "gorm" {
        return None;
    }
    if name == "gorm" || name == "//" {
        return None;
    }
    if name.chars().next()?.is_lowercase() {
        return None;
    }

    let go_type = parts[1].trim_start_matches('*');
    let mapped = match go_type {
        "string" => "string",
        "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16" | "uint32"
        | "uint64" => "int",
        "float32" | "float64" => "float",
        "bool" => "bool",
        "time.Time" => "datetime",
        _ if go_type.starts_with("[]") => "array",
        _ if go_type == "uuid.UUID" || go_type == "UUID" => "uuid",
        _ if go_type == "datatypes.JSON" || go_type == "JSON" => "json",
        _ => "FK",
    };

    Some((name.to_string(), mapped.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gorm_basic_struct() {
        let content = r#"
package models

import "gorm.io/gorm"

type User struct {
    gorm.Model
    Name   string
    Email  string
    Age    int
    Score  float64
    Active bool
}
"#;
        let mut models = Vec::new();
        parse_gorm_structs(content, &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "User");
        let fm: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(fm["Name"], "string");
        assert_eq!(fm["Age"], "int");
        assert_eq!(fm["Score"], "float");
        assert_eq!(fm["Active"], "bool");
    }

    #[test]
    fn test_gorm_tag_struct() {
        let content = r#"
type Product struct {
    ID    uint   `gorm:"primarykey"`
    Name  string `gorm:"size:100"`
    Price float32
}
"#;
        let mut models = Vec::new();
        parse_gorm_structs(content, &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Product");
    }

    #[test]
    fn test_gorm_non_gorm_struct_skipped() {
        let content = r#"
type Config struct {
    Host string
    Port int
}
"#;
        let mut models = Vec::new();
        parse_gorm_structs(content, &mut models);

        assert!(models.is_empty());
    }

    #[test]
    fn test_gorm_pointer_types() {
        let content = r#"
type Order struct {
    gorm.Model
    Total *float64
    User  *User
}
"#;
        let mut models = Vec::new();
        parse_gorm_structs(content, &mut models);

        assert_eq!(models.len(), 1);
        let fm: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(fm["Total"], "float");
        assert_eq!(fm["User"], "FK");
    }

    #[test]
    fn test_gorm_special_types() {
        let content = r#"
type Meta struct {
    gorm.Model
    Created time.Time
    Tags   []string
    Data   datatypes.JSON
    RefID  uuid.UUID
}
"#;
        let mut models = Vec::new();
        parse_gorm_structs(content, &mut models);

        let fm: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(fm["Created"], "datetime");
        assert_eq!(fm["Tags"], "array");
        assert_eq!(fm["Data"], "json");
        assert_eq!(fm["RefID"], "uuid");
    }

    #[test]
    fn test_scan_gorm_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("user.go"),
            r#"
package models
import "gorm.io/gorm"
type Account struct {
    gorm.Model
    Balance float64
}
"#,
        )
        .unwrap();

        let mut models = Vec::new();
        scan_gorm_models(dir.path(), &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Account");
    }
}
