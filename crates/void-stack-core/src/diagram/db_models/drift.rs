//! Drift (Dart/Flutter) table detection.

use super::DbModel;

pub(super) fn scan_drift_tables(dir: &std::path::Path, models: &mut Vec<DbModel>) {
    scan_drift_tables_recursive(dir, models, 0);
}

fn scan_drift_tables_recursive(dir: &std::path::Path, models: &mut Vec<DbModel>, depth: u32) {
    if depth > 4 {
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
                "node_modules"
                    | ".dart_tool"
                    | "build"
                    | ".pub-cache"
                    | "target"
                    | ".venv"
                    | "venv"
                    | "__pycache__"
                    | ".git"
            ) {
                continue;
            }
            scan_drift_tables_recursive(&path, models, depth + 1);
            continue;
        }
        if path.extension().map(|e| e == "dart").unwrap_or(false)
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            parse_drift_tables(&content, models);
        }
    }
}

fn parse_drift_tables(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("class ")
            && trimmed.contains("extends Table")
            && trimmed.ends_with('{')
        {
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
            i += 1;

            while i < lines.len() {
                let fl = lines[i].trim();
                if fl == "}" || (fl.starts_with("}") && !fl.contains("=>")) {
                    break;
                }

                if let Some((field_name, field_type)) = parse_drift_column(fl) {
                    fields.push((field_name, field_type));
                }

                i += 1;
            }

            if !fields.is_empty() {
                models.push(DbModel {
                    name: class_name,
                    fields,
                });
            }
            continue;
        }
        i += 1;
    }
}

fn parse_drift_column(line: &str) -> Option<(String, String)> {
    if !line.contains("Column") || !line.contains("get ") {
        return None;
    }

    let col_type = if line.contains("IntColumn") || line.contains("integer(") {
        "int"
    } else if line.contains("TextColumn") || line.contains("text(") {
        "string"
    } else if line.contains("BoolColumn") || line.contains("boolean(") {
        "bool"
    } else if line.contains("DateTimeColumn") || line.contains("dateTime(") {
        "datetime"
    } else if line.contains("RealColumn") || line.contains("real(") {
        "float"
    } else if line.contains("BlobColumn") || line.contains("blob(") {
        "binary"
    } else {
        "string"
    };

    let get_pos = line.find("get ")?;
    let after_get = &line[get_pos + 4..];
    let field_name = after_get.split_whitespace().next()?.to_string();

    if field_name.is_empty() || field_name == "=>" {
        return None;
    }

    Some((field_name, col_type.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drift_table() {
        let content = r#"
class Users extends Table {
  IntColumn get id => integer().autoIncrement()();
  TextColumn get name => text().withLength(min: 1, max: 100)();
  BoolColumn get active => boolean().withDefault(const Constant(true))();
  DateTimeColumn get createdAt => dateTime()();
  RealColumn get score => real()();
  BlobColumn get avatar => blob().nullable()();
}
"#;
        let mut models = Vec::new();
        parse_drift_tables(content, &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Users");
        let fm: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(fm["id"], "int");
        assert_eq!(fm["name"], "string");
        assert_eq!(fm["active"], "bool");
        assert_eq!(fm["createdAt"], "datetime");
        assert_eq!(fm["score"], "float");
        assert_eq!(fm["avatar"], "binary");
    }

    #[test]
    fn test_drift_multiple_tables() {
        let content = r#"
class Tasks extends Table {
  IntColumn get id => integer().autoIncrement()();
  TextColumn get title => text()();
}

class Categories extends Table {
  IntColumn get id => integer().autoIncrement()();
  TextColumn get name => text()();
}
"#;
        let mut models = Vec::new();
        parse_drift_tables(content, &mut models);

        assert_eq!(models.len(), 2);
    }

    #[test]
    fn test_drift_non_table_class_skipped() {
        let content = r#"
class MyWidget extends StatelessWidget {
  Widget build(BuildContext context) {}
}
"#;
        let mut models = Vec::new();
        parse_drift_tables(content, &mut models);

        assert!(models.is_empty());
    }

    #[test]
    fn test_drift_no_duplicate() {
        let content = r#"
class Items extends Table {
  IntColumn get id => integer()();
}
"#;
        let mut models = Vec::new();
        parse_drift_tables(content, &mut models);
        parse_drift_tables(content, &mut models);

        assert_eq!(models.len(), 1);
    }

    #[test]
    fn test_scan_drift_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tables.dart"),
            r#"
class Products extends Table {
  IntColumn get id => integer()();
  TextColumn get name => text()();
}
"#,
        )
        .unwrap();

        let mut models = Vec::new();
        scan_drift_tables(dir.path(), &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Products");
    }
}
