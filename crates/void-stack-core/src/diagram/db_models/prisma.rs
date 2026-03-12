//! Prisma schema detection.

use super::DbModel;

pub(super) fn scan_prisma_schema(dir: &std::path::Path, models: &mut Vec<DbModel>) {
    let prisma_path = dir.join("prisma").join("schema.prisma");
    let content = match std::fs::read_to_string(&prisma_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("model ") && trimmed.ends_with('{') {
            let model_name = trimmed
                .strip_prefix("model ")
                .and_then(|s| s.strip_suffix('{'))
                .unwrap_or("")
                .trim()
                .to_string();

            let mut fields = Vec::new();
            i += 1;

            while i < lines.len() {
                let field_line = lines[i].trim();
                if field_line == "}" {
                    break;
                }
                if field_line.is_empty() || field_line.starts_with("//") || field_line.starts_with("@@") {
                    i += 1;
                    continue;
                }

                let parts: Vec<&str> = field_line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let name = parts[0].to_string();
                    let prisma_type = parts[1]
                        .trim_end_matches('?')
                        .trim_end_matches("[]")
                        .to_lowercase();
                    let mapped = match prisma_type.as_str() {
                        "string" => "string",
                        "int" | "bigint" => "int",
                        "float" | "decimal" => "float",
                        "boolean" => "bool",
                        "datetime" => "datetime",
                        "json" => "json",
                        _ => "FK",
                    };
                    fields.push((name, mapped.to_string()));
                }

                i += 1;
            }

            if !fields.is_empty() {
                models.push(DbModel {
                    name: model_name,
                    fields,
                });
            }
        }
        i += 1;
    }
}
