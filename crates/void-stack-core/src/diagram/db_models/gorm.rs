//! GORM (Go) model detection.

use super::DbModel;

pub(super) fn scan_gorm_models(dir: &std::path::Path, models: &mut Vec<DbModel>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if entry.path().extension().map(|e| e == "go").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                parse_gorm_structs(&content, models);
            }
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

            if is_gorm_model && !fields.is_empty()
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
        "int" | "int8" | "int16" | "int32" | "int64"
        | "uint" | "uint8" | "uint16" | "uint32" | "uint64" => "int",
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
