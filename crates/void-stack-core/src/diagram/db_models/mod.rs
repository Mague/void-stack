//! Database model detection and ER diagram generation.
//!
//! Supports: SQLAlchemy, Django, Sequelize, GORM, Drift, Protobuf, Prisma.

mod python;
mod sequelize;
mod gorm;
mod drift;
mod proto;
mod prisma;

use std::path::Path;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;

/// A detected database model/table.
pub struct DbModel {
    pub name: String,
    pub fields: Vec<(String, String)>, // (name, type)
}

/// Scan and return raw DB model data (for use by multiple renderers).
pub fn scan_raw(project: &Project) -> Vec<DbModel> {
    let mut all_models: Vec<DbModel> = Vec::new();
    for svc in &project.services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let dir_clean = strip_win_prefix(dir);
        let dir_path = Path::new(&dir_clean);
        scan_models(dir_path, &mut all_models);
    }
    let root_clean = strip_win_prefix(&project.path);
    let root = Path::new(&root_clean);
    scan_models(root, &mut all_models);
    all_models
}

/// Generate a Mermaid ER diagram from detected database models.
pub fn generate(project: &Project) -> String {
    let all_models = scan_raw(project);

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
    python::scan_python_models(dir, models);
    sequelize::scan_sequelize_models(dir, models);
    gorm::scan_gorm_models(dir, models);
    prisma::scan_prisma_schema(dir, models);

    // Drift tables (only in specific dirs)
    for subdir in &["lib", "lib/src", "lib/database", "lib/data", "lib/db"] {
        let sub_path = dir.join(subdir);
        if sub_path.is_dir() {
            drift::scan_drift_tables(&sub_path, models);
        }
    }

    // Protobuf messages (only in specific dirs)
    for subdir in &["proto", "protos", "src/proto", "api/proto"] {
        let sub_path = dir.join(subdir);
        if sub_path.is_dir() {
            proto::scan_proto_messages(&sub_path, models);
        }
    }
    // Also check root for .proto files (non-recursive)
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().extension().map(|e| e == "proto").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    proto::parse_proto_messages(&content, models);
                }
            }
        }
    }

    // Scan known model subdirectories
    let model_dir_names = ["models", "db", "database", "schema", "entities", "entity"];
    for base in &["", "src", "app", "lib"] {
        let search_dir = if base.is_empty() { dir.to_path_buf() } else { dir.join(base) };
        if let Ok(entries) = std::fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if model_dir_names.contains(&name.as_str()) {
                        python::scan_python_models(&entry.path(), models);
                        sequelize::scan_sequelize_models(&entry.path(), models);
                        gorm::scan_gorm_models(&entry.path(), models);
                    }
                }
            }
        }
    }
}
