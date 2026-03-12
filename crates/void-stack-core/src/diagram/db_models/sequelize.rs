//! Sequelize (Node.js) model detection.

use super::DbModel;

pub(super) fn scan_sequelize_models(dir: &std::path::Path, models: &mut Vec<DbModel>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "js" | "ts" | "mjs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                parse_sequelize_define(&content, models);
                parse_sequelize_init(&content, models);
            }
        }
    }
}

/// Parse `sequelize.define('User', { ... })` or `Model.init({ ... }, ...)`
/// Also handles TypeScript generics: `.define<IInstance>('User', { ... })`
/// and multiline patterns where the model name is on the next line.
fn parse_sequelize_define(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        let is_define = trimmed.contains(".define(") || trimmed.contains(".define<");
        let is_init = trimmed.contains(".init(") && trimmed.contains("{");

        let model_name = if is_define {
            let name = extract_quoted_string(trimmed);
            if name.is_some() {
                name
            } else {
                let mut found = None;
                for lookahead in 1..=3 {
                    if i + lookahead < lines.len() {
                        found = extract_quoted_string(lines[i + lookahead].trim());
                        if found.is_some() {
                            i += lookahead;
                            break;
                        }
                    }
                }
                found
            }
        } else if is_init {
            trimmed.split(".init(").next()
                .and_then(|s| s.split_whitespace().last())
                .map(|s| s.to_string())
        } else {
            None
        };

        if let Some(name) = model_name {
            if name.is_empty() || models.iter().any(|m| m.name == name) {
                i += 1;
                continue;
            }

            let mut fields = Vec::new();
            i += 1;

            let mut brace_depth = 0;
            let mut found_fields_block = false;
            let mut current_field: Option<String> = None;
            let mut field_brace_depth: i32 = 0;

            while i < lines.len() {
                let fl = lines[i].trim();
                let open = fl.matches('{').count() as i32;
                let close = fl.matches('}').count() as i32;
                brace_depth += open;
                brace_depth -= close;

                if !found_fields_block {
                    if open > 0 {
                        found_fields_block = true;
                    }
                    i += 1;
                    continue;
                }

                if brace_depth <= 0 {
                    break;
                }

                // Try inline: fieldName: DataTypes.STRING
                if brace_depth == 1 && fl.contains("DataTypes.") && !fl.trim_start().starts_with("type") {
                    if let Some((field_name, field_type)) = parse_sequelize_field_inline(fl) {
                        fields.push((field_name, field_type));
                        i += 1;
                        continue;
                    }
                }

                // Nested object pattern
                if brace_depth >= 1 && fl.contains(": {") || fl.contains(":{") {
                    if let Some(colon_pos) = fl.find(':') {
                        let candidate = fl[..colon_pos].trim().trim_matches('\'').trim_matches('"').to_string();
                        if !candidate.is_empty()
                            && !candidate.starts_with("//")
                            && !matches!(candidate.as_str(), "type" | "allowNull" | "defaultValue"
                                | "primaryKey" | "autoIncrement" | "references" | "get" | "set"
                                | "validate" | "unique" | "comment" | "field" | "onDelete" | "onUpdate")
                        {
                            if let Some(dt) = extract_datatype_from_line(fl) {
                                fields.push((candidate, dt));
                            } else {
                                current_field = Some(candidate);
                                field_brace_depth = brace_depth;
                            }
                        }
                    }
                } else if current_field.is_some() && fl.contains("DataTypes.") || fl.contains("DataType.") {
                    if fl.trim_start().starts_with("type:") || fl.trim_start().starts_with("type :") {
                        if let Some(dt) = extract_datatype_from_line(fl) {
                            if let Some(name) = current_field.take() {
                                fields.push((name, dt));
                            }
                        }
                    }
                }

                if let Some(ref _f) = current_field {
                    if brace_depth <= field_brace_depth - 1 {
                        current_field = None;
                    }
                }

                i += 1;
            }

            if !fields.is_empty() {
                models.push(DbModel { name, fields });
            }
            continue;
        }
        i += 1;
    }
}

/// Extract the first quoted string ('xxx' or "xxx") from a line.
fn extract_quoted_string(line: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        if let Some(start) = line.find(quote) {
            let rest = &line[start + 1..];
            if let Some(end) = rest.find(quote) {
                let val = &rest[..end];
                if !val.is_empty() && !val.contains(' ') {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Extract DataTypes.XXX from a line and map to a simple type.
fn extract_datatype_from_line(line: &str) -> Option<String> {
    let dt_pos = line.find("DataTypes.").or_else(|| line.find("DataType."))?;
    let after = &line[dt_pos..];
    let type_str = after.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_')
        .next()
        .unwrap_or("");

    let mapped = if type_str.contains("STRING") || type_str.contains("TEXT") || type_str.contains("CHAR") {
        "string"
    } else if type_str.contains("INTEGER") || type_str.contains("BIGINT") || type_str.contains("SMALLINT") {
        "int"
    } else if type_str.contains("FLOAT") || type_str.contains("DOUBLE") || type_str.contains("DECIMAL") || type_str.contains("REAL") {
        "float"
    } else if type_str.contains("BOOLEAN") {
        "bool"
    } else if type_str.contains("DATE") {
        "datetime"
    } else if type_str.contains("JSON") {
        "json"
    } else if type_str.contains("UUID") {
        "uuid"
    } else if type_str.contains("BLOB") || type_str.contains("BINARY") {
        "binary"
    } else if type_str.contains("ENUM") {
        "enum"
    } else if type_str.contains("ARRAY") {
        "array"
    } else {
        "string"
    };

    Some(mapped.to_string())
}

/// Parse inline Sequelize field: `fieldName: DataTypes.STRING,`
fn parse_sequelize_field_inline(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim().trim_end_matches(',');
    if !trimmed.contains("DataTypes.") && !trimmed.contains("DataType.") {
        return None;
    }
    let colon_pos = trimmed.find(':')?;
    let name = trimmed[..colon_pos].trim().trim_matches('\'').trim_matches('"').to_string();
    if name == "type" || name == "allowNull" || name == "defaultValue"
        || name == "primaryKey" || name == "autoIncrement" || name == "references"
        || name.starts_with("//") || name.is_empty() {
        return None;
    }
    let dt = extract_datatype_from_line(line)?;
    Some((name, dt))
}

/// Parse Sequelize class-based models: `class User extends Model { ... }`
fn parse_sequelize_init(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("class ") && trimmed.contains("extends Model") {
            let class_name = trimmed
                .strip_prefix("class ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("")
                .to_string();

            if class_name.is_empty() || models.iter().any(|m| m.name == class_name) {
                i += 1;
                continue;
            }

            let mut fields = Vec::new();
            let mut brace_depth = 0;
            let start = i;
            i += 1;

            while i < lines.len() {
                let fl = lines[i].trim();
                brace_depth += fl.matches('{').count() as i32;
                brace_depth -= fl.matches('}').count() as i32;

                if let Some((field_name, field_type)) = parse_sequelize_field_inline(fl) {
                    fields.push((field_name, field_type));
                }

                if brace_depth < 0 && i > start + 1 {
                    break;
                }
                i += 1;
            }

            if !fields.is_empty() {
                models.push(DbModel { name: class_name, fields });
            }
            continue;
        }
        i += 1;
    }
}
