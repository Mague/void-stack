//! Disk space scanner: detects heavy directories (node_modules, venv, caches,
//! AI models, build artifacts) and provides cleanup capabilities.

use std::path::{Path, PathBuf};
use serde::Serialize;

/// A detected heavy directory or cache.
#[derive(Debug, Clone, Serialize)]
pub struct SpaceEntry {
    /// Display name (e.g., "node_modules", "HuggingFace cache")
    pub name: String,
    /// Category for grouping
    pub category: SpaceCategory,
    /// Absolute path
    pub path: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// Human-readable size (e.g., "1.2 GB")
    pub size_human: String,
    /// Whether it's safe to delete (can be reinstalled/rebuilt)
    pub deletable: bool,
    /// How to restore after deletion
    pub restore_hint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SpaceCategory {
    /// Per-project dependency dirs (node_modules, venv, target, etc.)
    Dependencies,
    /// Build artifacts (target/, build/, dist/, .dart_tool/)
    BuildArtifacts,
    /// Global caches (pip, npm, go modules, pub)
    GlobalCache,
    /// AI model storage (Ollama, HuggingFace, LM Studio)
    AiModels,
}

impl std::fmt::Display for SpaceCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpaceCategory::Dependencies => write!(f, "Dependencias"),
            SpaceCategory::BuildArtifacts => write!(f, "Build"),
            SpaceCategory::GlobalCache => write!(f, "Caché global"),
            SpaceCategory::AiModels => write!(f, "Modelos AI"),
        }
    }
}

/// Scan a project directory for heavy subdirectories.
pub fn scan_project(project_path: &Path) -> Vec<SpaceEntry> {
    let mut entries = Vec::new();

    // Per-project heavy dirs
    let project_targets = [
        ("node_modules", SpaceCategory::Dependencies, true, "npm install"),
        (".venv", SpaceCategory::Dependencies, true, "python -m venv .venv && pip install -r requirements.txt"),
        ("venv", SpaceCategory::Dependencies, true, "python -m venv venv && pip install -r requirements.txt"),
        ("env", SpaceCategory::Dependencies, true, "python -m venv env"),
        ("target", SpaceCategory::BuildArtifacts, true, "cargo build"),
        ("build", SpaceCategory::BuildArtifacts, true, "rebuild project"),
        ("dist", SpaceCategory::BuildArtifacts, true, "rebuild project"),
        (".dart_tool", SpaceCategory::BuildArtifacts, true, "flutter pub get"),
        (".flutter-plugins", SpaceCategory::BuildArtifacts, true, "flutter pub get"),
        ("__pycache__", SpaceCategory::BuildArtifacts, true, "auto-regenerated on run"),
        (".next", SpaceCategory::BuildArtifacts, true, "next build"),
        (".nuxt", SpaceCategory::BuildArtifacts, true, "nuxt build"),
    ];

    for (dir_name, category, deletable, restore) in &project_targets {
        scan_recursive_for(project_path, dir_name, *category, *deletable, restore, &mut entries, 3);
    }

    entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    entries
}

/// Scan global caches and AI model storage.
pub fn scan_global() -> Vec<SpaceEntry> {
    let mut entries = Vec::new();

    let home = dirs::home_dir().unwrap_or_default();
    let local_app = dirs::data_local_dir().unwrap_or_default();
    let app_data = dirs::config_dir().unwrap_or_default();

    // Global caches
    let global_targets: Vec<(&str, PathBuf, SpaceCategory, &str)> = vec![
        // npm cache
        ("npm cache", app_data.join("npm-cache"), SpaceCategory::GlobalCache, "npm cache clean --force"),
        // pip cache
        ("pip cache", local_app.join("pip").join("cache"), SpaceCategory::GlobalCache, "pip cache purge"),
        // Go module cache
        ("Go modules", home.join("go").join("pkg").join("mod"), SpaceCategory::GlobalCache, "go clean -modcache"),
        // Cargo registry
        ("Cargo registry", home.join(".cargo").join("registry"), SpaceCategory::GlobalCache, "cargo cache --autoclean (requires cargo-cache)"),
        // Flutter/Dart pub cache
        ("Dart pub cache", local_app.join("Pub").join("Cache"), SpaceCategory::GlobalCache, "dart pub cache clean"),
        // Gradle cache (Android/Flutter)
        ("Gradle cache", home.join(".gradle").join("caches"), SpaceCategory::GlobalCache, "gradle --stop && rm -rf ~/.gradle/caches"),

        // AI Models
        ("Ollama models", home.join(".ollama").join("models"), SpaceCategory::AiModels, "ollama pull <model>"),
        ("HuggingFace cache", home.join(".cache").join("huggingface"), SpaceCategory::AiModels, "huggingface-cli download <model>"),
        ("LM Studio models", home.join(".cache").join("lm-studio"), SpaceCategory::AiModels, "download from LM Studio app"),
        // torch hub cache (PyTorch models)
        ("PyTorch hub", home.join(".cache").join("torch"), SpaceCategory::AiModels, "re-download on first use"),
    ];

    for (name, path, category, restore) in &global_targets {
        if path.exists() {
            let size = dir_size(path);
            if size > 1_000_000 { // Only show if > 1MB
                entries.push(SpaceEntry {
                    name: name.to_string(),
                    category: *category,
                    path: path.to_string_lossy().to_string(),
                    size_bytes: size,
                    size_human: format_size(size),
                    deletable: true,
                    restore_hint: restore.to_string(),
                });
            }
        }
    }

    entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    entries
}

/// Delete a directory and return the freed size.
pub fn delete_entry(path: &str) -> Result<u64, String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("La ruta no existe".to_string());
    }

    // Safety: only allow deleting known safe directories
    let dir_name = p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let safe_names = [
        "node_modules", ".venv", "venv", "env", "target", "build", "dist",
        ".dart_tool", ".flutter-plugins", "__pycache__", ".next", ".nuxt",
        // Global caches
        "npm-cache", "cache", "Cache", "caches", "mod", "registry",
        // AI models
        "models", "huggingface", "lm-studio", "torch",
    ];

    if !safe_names.contains(&dir_name) {
        return Err(format!("No se permite eliminar '{}' por seguridad", dir_name));
    }

    let size = dir_size(p);
    std::fs::remove_dir_all(p).map_err(|e| format!("Error eliminando: {}", e))?;
    Ok(size)
}

/// Recursively search for a named directory up to max_depth levels.
fn scan_recursive_for(
    base: &Path,
    target_name: &str,
    category: SpaceCategory,
    deletable: bool,
    restore: &str,
    entries: &mut Vec<SpaceEntry>,
    max_depth: u32,
) {
    scan_recursive_inner(base, target_name, category, deletable, restore, entries, 0, max_depth);
}

fn scan_recursive_inner(
    dir: &Path,
    target_name: &str,
    category: SpaceCategory,
    deletable: bool,
    restore: &str,
    entries: &mut Vec<SpaceEntry>,
    depth: u32,
    max_depth: u32,
) {
    if depth > max_depth {
        return;
    }

    let target_path = dir.join(target_name);
    if target_path.is_dir() {
        let size = dir_size(&target_path);
        if size > 500_000 { // Only show if > 500KB
            // Build display name relative to project
            let display = if depth == 0 {
                target_name.to_string()
            } else {
                // Show relative path from the initial base
                target_path.to_string_lossy().to_string()
            };

            entries.push(SpaceEntry {
                name: display,
                category,
                path: target_path.to_string_lossy().to_string(),
                size_bytes: size,
                size_human: format_size(size),
                deletable,
                restore_hint: restore.to_string(),
            });
        }
        return; // Don't recurse into the target itself
    }

    // Recurse into subdirectories (but skip known heavy dirs)
    let skip = ["node_modules", ".git", "target", ".venv", "venv", ".dart_tool"];
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if !skip.contains(&name) && !name.starts_with('.') {
                    scan_recursive_inner(&path, target_name, category, deletable, restore, entries, depth + 1, max_depth);
                }
            }
        }
    }
}

/// Calculate total size of a directory recursively.
fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    dir_size_inner(path, &mut total, 0);
    total
}

fn dir_size_inner(path: &Path, total: &mut u64, depth: u32) {
    // Limit recursion depth for safety
    if depth > 20 {
        return;
    }

    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.is_file() {
            *total += meta.len();
        } else if meta.is_dir() {
            dir_size_inner(&entry.path(), total, depth + 1);
        }
    }
}

/// Format bytes into human-readable string.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(1_500_000), "1.4 MB");
        assert_eq!(format_size(2_500_000_000), "2.3 GB");
    }

    #[test]
    fn test_scan_project_no_crash() {
        // Should not crash on a non-existent path
        let entries = scan_project(Path::new("nonexistent_path_xyz"));
        assert!(entries.is_empty());
    }
}
