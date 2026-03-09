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
            // Mermaid erDiagram: FK/M2M are key annotations, not types
            // Format: type name or type name FK "comment"
            if field_type == "FK" || field_type == "M2M" {
                lines.push(format!("        string {} {}", field_name, field_type));
            } else {
                lines.push(format!("        {} {}", field_type, field_name));
            }
        }
        lines.push("    }".to_string());
    }

    lines.push("```".to_string());
    lines.join("\n")
}

fn scan_models(dir: &Path, models: &mut Vec<DbModel>) {
    // Scan Python files for SQLAlchemy/Django models
    scan_python_models(dir, models);

    // Scan JS/TS files for Sequelize models
    scan_sequelize_models(dir, models);

    // Scan Go files for GORM models
    scan_gorm_models(dir, models);

    // Scan for Prisma schema
    scan_prisma_schema(dir, models);

    // Scan Dart files for Drift tables (only in specific dirs, not root — avoid node_modules etc.)
    for subdir in &["lib", "lib/src", "lib/database", "lib/data", "lib/db"] {
        let sub_path = dir.join(subdir);
        if sub_path.is_dir() {
            scan_drift_tables(&sub_path, models);
        }
    }

    // Scan Protobuf files (only in specific dirs)
    for subdir in &["proto", "protos", "src/proto", "api/proto"] {
        let sub_path = dir.join(subdir);
        if sub_path.is_dir() {
            scan_proto_messages(&sub_path, models);
        }
    }
    // Also check root for .proto files (non-recursive)
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().extension().map(|e| e == "proto").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    parse_proto_messages(&content, models);
                }
            }
        }
    }

    // Scan known model subdirectories (case-insensitive match for WSL/ext4)
    let model_dir_names = ["models", "db", "database", "schema", "entities", "entity"];
    for base in &["", "src", "app", "lib"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        if let Ok(entries) = std::fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if model_dir_names.contains(&name.as_str()) {
                        scan_python_models(&entry.path(), models);
                        scan_sequelize_models(&entry.path(), models);
                        scan_gorm_models(&entry.path(), models);
                    }
                }
            }
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

// ─── Sequelize (Node.js) ─────────────────────────────────────────────

fn scan_sequelize_models(dir: &Path, models: &mut Vec<DbModel>) {
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

        // Pattern 1: .define('User', { or .define<Type>('User', {
        // Pattern 2: ModelName.init({
        let is_define = trimmed.contains(".define(") || trimmed.contains(".define<");
        let is_init = trimmed.contains(".init(") && trimmed.contains("{");

        let model_name = if is_define {
            // Try to extract name from this line first
            let name = extract_quoted_string(trimmed);
            if name.is_some() {
                name
            } else {
                // Name might be on the next line(s) — look ahead up to 3 lines
                let mut found = None;
                for lookahead in 1..=3 {
                    if i + lookahead < lines.len() {
                        found = extract_quoted_string(lines[i + lookahead].trim());
                        if found.is_some() {
                            i += lookahead; // advance to the line with the name
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

            // Find the opening { of the fields object
            let mut brace_depth = 0;
            let mut found_fields_block = false;
            // Track current field name for nested objects: fieldName: { type: DataTypes.XXX }
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

                // End of fields block (back to depth 0 or negative)
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

                // Nested object pattern: fieldName: { ... type: DataTypes.XXX ... }
                // Single-line: name: { allowNull: false, type: DataTypes.STRING }
                if brace_depth >= 1 && fl.contains(": {") || fl.contains(":{") {
                    // Extract field name before the colon
                    if let Some(colon_pos) = fl.find(':') {
                        let candidate = fl[..colon_pos].trim().trim_matches('\'').trim_matches('"').to_string();
                        if !candidate.is_empty()
                            && !candidate.starts_with("//")
                            && !matches!(candidate.as_str(), "type" | "allowNull" | "defaultValue"
                                | "primaryKey" | "autoIncrement" | "references" | "get" | "set"
                                | "validate" | "unique" | "comment" | "field" | "onDelete" | "onUpdate")
                        {
                            // Check if type is on same line
                            if let Some(dt) = extract_datatype_from_line(fl) {
                                fields.push((candidate, dt));
                            } else {
                                current_field = Some(candidate);
                                field_brace_depth = brace_depth;
                            }
                        }
                    }
                } else if current_field.is_some() && fl.contains("DataTypes.") || fl.contains("DataType.") {
                    // We're inside a field's nested object, look for type: DataTypes.XXX
                    if fl.trim_start().starts_with("type:") || fl.trim_start().starts_with("type :") {
                        if let Some(dt) = extract_datatype_from_line(fl) {
                            if let Some(name) = current_field.take() {
                                fields.push((name, dt));
                            }
                        }
                    }
                }

                // If we've closed back to field level, clear current_field
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
/// with field definitions in a static init or associate method.
fn parse_sequelize_init(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // class User extends Model {
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

            // Look for init call inside the class
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

                // End of class
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

// ─── GORM (Go) ──────────────────────────────────────────────────────

fn scan_gorm_models(dir: &Path, models: &mut Vec<DbModel>) {
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

        // type User struct {
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

                // Check for gorm.Model embed
                if fl == "gorm.Model" || fl.starts_with("gorm.Model") {
                    is_gorm_model = true;
                    i += 1;
                    continue;
                }

                // Check for gorm:"..." tag
                if fl.contains("gorm:\"") || fl.contains("`gorm:") {
                    is_gorm_model = true;
                }

                // Parse Go struct field: Name string `gorm:"..." json:"..."`
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
    // Skip embedded structs and unexported fields
    if name.chars().next()?.is_lowercase() && name != "gorm" {
        return None;
    }
    if name == "gorm" || name == "//" {
        return None;
    }
    // Skip if first char is lowercase (unexported)
    if name.chars().next()?.is_lowercase() {
        return None;
    }

    let go_type = parts[1].trim_start_matches('*'); // handle *string, *int, etc.
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
        _ => "FK", // likely a relation
    };

    Some((name.to_string(), mapped.to_string()))
}

// ─── Drift (Dart/Flutter) ────────────────────────────────────────────

fn scan_drift_tables(dir: &Path, models: &mut Vec<DbModel>) {
    scan_drift_tables_recursive(dir, models, 0);
}

fn scan_drift_tables_recursive(dir: &Path, models: &mut Vec<DbModel>, depth: u32) {
    if depth > 4 { return; } // Limit recursion depth
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip heavy directories
            if matches!(name.as_str(), "node_modules" | ".dart_tool" | "build" | ".pub-cache"
                | "target" | ".venv" | "venv" | "__pycache__" | ".git") {
                continue;
            }
            scan_drift_tables_recursive(&path, models, depth + 1);
            continue;
        }
        if path.extension().map(|e| e == "dart").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                parse_drift_tables(&content, models);
            }
        }
    }
}

/// Parse Drift table classes:
/// ```dart
/// class TodoItems extends Table {
///   IntColumn get id => integer().autoIncrement()();
///   TextColumn get title => text().withLength(min: 6, max: 32)();
///   BoolColumn get completed => boolean().withDefault(const Constant(false))();
/// }
/// ```
fn parse_drift_tables(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // class TodoItems extends Table {
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

                // IntColumn get id => integer()...
                // TextColumn get name => text()...
                // BoolColumn get active => boolean()...
                // DateTimeColumn get createdAt => dateTime()...
                // RealColumn get price => real()...
                // BlobColumn get data => blob()...
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
    // Pattern: XxxColumn get fieldName => ...
    if !line.contains("Column") || !line.contains("get ") {
        return None;
    }

    // Extract the column type from the type annotation
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

    // Extract field name: "get fieldName =>"
    let get_pos = line.find("get ")?;
    let after_get = &line[get_pos + 4..];
    let field_name = after_get.split_whitespace().next()?.to_string();

    if field_name.is_empty() || field_name == "=>" {
        return None;
    }

    Some((field_name, col_type.to_string()))
}

// ─── Protobuf ───────────────────────────────────────────────────────

fn scan_proto_messages(dir: &Path, models: &mut Vec<DbModel>) {
    scan_proto_messages_recursive(dir, models, 0);
}

fn scan_proto_messages_recursive(dir: &Path, models: &mut Vec<DbModel>, depth: u32) {
    if depth > 3 { return; } // Limit recursion depth
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if matches!(name.as_str(), "node_modules" | "target" | ".git" | "build"
                | ".venv" | "venv" | "__pycache__") {
                continue;
            }
            scan_proto_messages_recursive(&path, models, depth + 1);
            continue;
        }
        if path.extension().map(|e| e == "proto").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                parse_proto_messages(&content, models);
            }
        }
    }
}

/// Parse protobuf message definitions:
/// ```proto
/// message User {
///   int32 id = 1;
///   string name = 2;
///   bool active = 3;
/// }
/// ```
fn parse_proto_messages(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // message UserRequest {
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
                if fl.is_empty() || fl.starts_with("//") || fl.starts_with("reserved")
                    || fl.starts_with("option") || fl.starts_with("oneof")
                    || fl.starts_with("message ") || fl.starts_with("enum ")
                {
                    i += 1;
                    continue;
                }

                // type field_name = number;
                // repeated type field_name = number;
                // optional type field_name = number;
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

    // Skip map<> fields for simplicity
    if parts[0] == "map" || parts[0].starts_with("map<") {
        // map<string, string> field_name = N;
        let name = parts.iter().find(|p| p.contains('='))
            .and_then(|_| parts.get(parts.len().saturating_sub(3)))
            .map(|s| s.to_string())?;
        return Some((name, "map".to_string()));
    }

    let (proto_type, field_name) = if parts[0] == "repeated" || parts[0] == "optional" {
        // repeated/optional type name = N
        if parts.len() < 4 { return None; }
        (parts[1], parts[2])
    } else {
        // type name = N
        (parts[0], parts[1])
    };

    let mapped = match proto_type {
        "string" => "string",
        "int32" | "int64" | "sint32" | "sint64" | "uint32" | "uint64"
        | "fixed32" | "fixed64" | "sfixed32" | "sfixed64" => "int",
        "float" | "double" => "float",
        "bool" => "bool",
        "bytes" => "binary",
        "google.protobuf.Timestamp" => "datetime",
        _ => "FK", // Likely a message reference
    };

    Some((field_name.to_string(), mapped.to_string()))
}

// ─── Prisma (Node.js / Python) ──────────────────────────────────────

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
