//! Database model detection and ER diagram generation.

use std::path::Path;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// A detected database model/table.
struct DbModel {
    name: String,
    fields: Vec<(String, String)>, // (name, type)
}

/// Generate a Mermaid ER diagram from detected database models.
pub fn generate(project: &Project) -> String {
    let mut all_models: Vec<DbModel> = Vec::new();

    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);
        scan_models(dir_path, &mut all_models);
    }

    // Also scan project root
    let root_clean = strip_win_prefix(&project.path);
    let root = Path::new(&root_clean);
    scan_models(root, &mut all_models);

    if all_models.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "```mermaid".to_string(),
        "erDiagram".to_string(),
    ];

    for model in &all_models {
        lines.push(format!("    {} {{", model.name));
        for (field_name, field_type) in &model.fields {
            lines.push(format!("        {} {}", field_type, field_name));
        }
        lines.push("    }".to_string());
    }

    lines.push("```".to_string());
    lines.join("\n")
}

fn scan_models(dir: &Path, models: &mut Vec<DbModel>) {
    // Scan Python files for SQLAlchemy/Django models
    scan_python_models(dir, models);

    // Scan for Prisma schema
    scan_prisma_schema(dir, models);

    // Scan subdirectories
    for subdir in &["models", "db", "database", "schema"] {
        let sub_path = dir.join(subdir);
        if sub_path.is_dir() {
            scan_python_models(&sub_path, models);
        }
    }
}

fn scan_python_models(dir: &Path, models: &mut Vec<DbModel>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if entry.path().extension().map(|e| e == "py").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                parse_sqlalchemy_models(&content, models);
                parse_django_models(&content, models);
            }
        }
    }
}

fn parse_sqlalchemy_models(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // class User(Base): or class User(db.Model):
        if trimmed.starts_with("class ") && (trimmed.contains("(Base)") || trimmed.contains("db.Model")) {
            let class_name = trimmed
                .strip_prefix("class ")
                .and_then(|s| s.split('(').next())
                .unwrap_or("")
                .trim()
                .to_string();

            if class_name.is_empty() || models.iter().any(|m| m.name == class_name) {
                i += 1;
                continue;
            }

            let mut fields = Vec::new();
            i += 1;

            while i < lines.len() {
                let field_line = lines[i].trim();
                if field_line.is_empty() || field_line.starts_with('#') {
                    i += 1;
                    continue;
                }
                // Stop at next class or non-indented line
                if !lines[i].starts_with(' ') && !lines[i].starts_with('\t') && !field_line.is_empty() {
                    break;
                }

                // name = Column(String, ...) or name = Column(Integer, ...)
                if field_line.contains("Column(") || field_line.contains("column(") {
                    if let Some((name, col_type)) = parse_column_def(field_line) {
                        fields.push((name, col_type));
                    }
                }
                // name: Mapped[str] = mapped_column(...)
                if field_line.contains("Mapped[") || field_line.contains("mapped_column") {
                    if let Some((name, col_type)) = parse_mapped_column(field_line) {
                        fields.push((name, col_type));
                    }
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

fn parse_django_models(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // class Post(models.Model):
        if trimmed.starts_with("class ") && trimmed.contains("models.Model") {
            let class_name = trimmed
                .strip_prefix("class ")
                .and_then(|s| s.split('(').next())
                .unwrap_or("")
                .trim()
                .to_string();

            if class_name.is_empty() || models.iter().any(|m| m.name == class_name) {
                i += 1;
                continue;
            }

            let mut fields = Vec::new();
            i += 1;

            while i < lines.len() {
                let field_line = lines[i].trim();
                if !lines[i].starts_with(' ') && !lines[i].starts_with('\t') && !field_line.is_empty() {
                    break;
                }

                // title = models.CharField(max_length=200)
                if field_line.contains("models.") && field_line.contains('=') {
                    if let Some((name, field_type)) = parse_django_field(field_line) {
                        fields.push((name, field_type));
                    }
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

fn parse_column_def(line: &str) -> Option<(String, String)> {
    let eq_pos = line.find('=')?;
    let name = line[..eq_pos].trim().to_string();
    if name.starts_with('_') || name.starts_with('#') {
        return None;
    }

    let rest = &line[eq_pos + 1..];
    let col_type = if rest.contains("String") || rest.contains("Text") {
        "string"
    } else if rest.contains("Integer") || rest.contains("BigInteger") {
        "int"
    } else if rest.contains("Float") || rest.contains("Numeric") {
        "float"
    } else if rest.contains("Boolean") {
        "bool"
    } else if rest.contains("DateTime") || rest.contains("Date") {
        "datetime"
    } else if rest.contains("ForeignKey") {
        "FK"
    } else if rest.contains("JSON") {
        "json"
    } else {
        "string"
    };

    Some((name, col_type.to_string()))
}

fn parse_mapped_column(line: &str) -> Option<(String, String)> {
    // name: Mapped[str] = mapped_column(...)
    let colon_pos = line.find(':')?;
    let name = line[..colon_pos].trim().to_string();

    let mapped_type = if line.contains("Mapped[str]") || line.contains("Mapped[String]") {
        "string"
    } else if line.contains("Mapped[int]") {
        "int"
    } else if line.contains("Mapped[float]") {
        "float"
    } else if line.contains("Mapped[bool]") {
        "bool"
    } else if line.contains("Mapped[datetime]") || line.contains("Mapped[date]") {
        "datetime"
    } else {
        "string"
    };

    Some((name, mapped_type.to_string()))
}

fn parse_django_field(line: &str) -> Option<(String, String)> {
    let eq_pos = line.find('=')?;
    let name = line[..eq_pos].trim().to_string();
    if name.starts_with('_') || name.starts_with('#') || name == "class" || name == "Meta" {
        return None;
    }

    let rest = &line[eq_pos + 1..];
    let field_type = if rest.contains("CharField") || rest.contains("TextField") || rest.contains("SlugField") {
        "string"
    } else if rest.contains("IntegerField") || rest.contains("BigIntegerField") || rest.contains("PositiveIntegerField") {
        "int"
    } else if rest.contains("FloatField") || rest.contains("DecimalField") {
        "float"
    } else if rest.contains("BooleanField") {
        "bool"
    } else if rest.contains("DateTimeField") || rest.contains("DateField") {
        "datetime"
    } else if rest.contains("ForeignKey") || rest.contains("OneToOneField") {
        "FK"
    } else if rest.contains("ManyToManyField") {
        "M2M"
    } else if rest.contains("JSONField") {
        "json"
    } else if rest.contains("FileField") || rest.contains("ImageField") {
        "file"
    } else {
        "string"
    };

    Some((name, field_type.to_string()))
}

fn scan_prisma_schema(dir: &Path, models: &mut Vec<DbModel>) {
    let prisma_path = dir.join("prisma").join("schema.prisma");
    let content = match std::fs::read_to_string(&prisma_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // model User {
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
                        _ => "FK", // Likely a relation
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
