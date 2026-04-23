//! Unified `.void-config` (TOML) project configuration.
//!
//! Replaces the legacy `.voidignore` + `.void-audit-ignore` pair with a
//! single file that covers indexing, auditing, analysis, diagrams, and AI.
//! Falls back to legacy files transparently if `.void-config` doesn't exist.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Top-level project configuration loaded from `.void-config`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub audit: AuditConfig,
    #[serde(default)]
    pub analysis: AnalysisConfig,
    #[serde(default)]
    pub diagram: DiagramConfig,
    #[serde(default)]
    pub ai: AiConfig,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Glob patterns to exclude from the semantic index.
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Suppression rules: finding-id glob + file-path glob.
    #[serde(default)]
    pub suppress: Vec<SuppressEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuppressEntry {
    pub rule: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    /// Cyclomatic complexity threshold for flagging (default: 10).
    #[serde(default = "default_cc")]
    pub cc_threshold: usize,
    /// LOC threshold for fat-controller detection (default: 200).
    #[serde(default = "default_fat_loc")]
    pub fat_controller_loc: usize,
}

fn default_cc() -> usize {
    10
}
fn default_fat_loc() -> usize {
    200
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            cc_threshold: default_cc(),
            fat_controller_loc: default_fat_loc(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramConfig {
    /// "mermaid" or "drawio".
    #[serde(default = "default_diagram_format")]
    pub default_format: String,
}

fn default_diagram_format() -> String {
    "drawio".into()
}

impl Default for DiagramConfig {
    fn default() -> Self {
        Self {
            default_format: default_diagram_format(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Ollama model override.
    #[serde(default)]
    pub default_model: Option<String>,
}

// ── Load / Save ─────────────────────────────────────────────

const CONFIG_FILENAME: &str = ".void-config";

impl ProjectConfig {
    /// Load from `.void-config` (TOML). Falls back to legacy files if absent.
    pub fn load(project_root: &Path) -> Self {
        let new_path = project_root.join(CONFIG_FILENAME);
        if new_path.exists()
            && let Ok(content) = std::fs::read_to_string(&new_path)
        {
            if let Ok(cfg) = toml::from_str::<ProjectConfig>(&content) {
                return cfg;
            }
            tracing::warn!("Failed to parse .void-config, falling back to legacy files");
        }

        // Legacy fallback
        let mut cfg = ProjectConfig::default();

        // .voidignore → index.ignore
        let voidignore = project_root.join(".voidignore");
        if voidignore.exists()
            && let Ok(content) = std::fs::read_to_string(&voidignore)
        {
            cfg.index.ignore = content
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .map(String::from)
                .collect();
        }

        // .void-audit-ignore → audit.suppress
        let audit_ignore = project_root.join(".void-audit-ignore");
        if audit_ignore.exists()
            && let Ok(content) = std::fs::read_to_string(&audit_ignore)
        {
            cfg.audit.suppress = content
                .lines()
                .filter_map(|line| {
                    let t = line.trim();
                    if t.is_empty() || t.starts_with('#') {
                        return None;
                    }
                    let mut parts = t.splitn(2, char::is_whitespace);
                    let rule = parts.next()?.trim().to_string();
                    let path = parts.next()?.trim().to_string();
                    if rule.is_empty() || path.is_empty() {
                        return None;
                    }
                    Some(SuppressEntry { rule, path })
                })
                .collect();
        }

        cfg
    }

    /// Persist to `.void-config` TOML with a header comment.
    pub fn save(&self, project_root: &Path) -> Result<(), String> {
        let path = project_root.join(CONFIG_FILENAME);
        let toml_str =
            toml::to_string_pretty(self).map_err(|e| format!("TOML serialize: {}", e))?;
        let header = "# .void-config — void-stack project configuration\n\
                      # Documentation: https://void-stack.dev/config\n\n";
        std::fs::write(&path, format!("{}{}", header, toml_str)).map_err(|e| e.to_string())
    }

    /// True if a `.void-config` file exists at the project root.
    pub fn exists(project_root: &Path) -> bool {
        project_root.join(CONFIG_FILENAME).exists()
    }
}

// ── Migration ───────────────────────────────────────────────

/// Result of migrating legacy config files.
#[derive(Debug, Clone, Serialize)]
pub enum MigrationReport {
    AlreadyMigrated,
    NothingToMigrate,
    Migrated {
        voidignore_rules: usize,
        audit_suppressions: usize,
    },
}

/// Migrate `.voidignore` + `.void-audit-ignore` into `.void-config`.
/// Renames legacy files to `.backup` so they don't shadow the new one.
pub fn migrate_legacy_config(project_root: &Path) -> Result<MigrationReport, String> {
    if ProjectConfig::exists(project_root) {
        return Ok(MigrationReport::AlreadyMigrated);
    }

    let voidignore = project_root.join(".voidignore");
    let audit_ignore = project_root.join(".void-audit-ignore");

    if !voidignore.exists() && !audit_ignore.exists() {
        return Ok(MigrationReport::NothingToMigrate);
    }

    let cfg = ProjectConfig::load(project_root);
    let vi_count = cfg.index.ignore.len();
    let au_count = cfg.audit.suppress.len();
    cfg.save(project_root)?;

    // Backup legacy files
    if voidignore.exists() {
        let _ = std::fs::rename(&voidignore, project_root.join(".voidignore.backup"));
    }
    if audit_ignore.exists() {
        let _ = std::fs::rename(
            &audit_ignore,
            project_root.join(".void-audit-ignore.backup"),
        );
    }

    Ok(MigrationReport::Migrated {
        voidignore_rules: vi_count,
        audit_suppressions: au_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_void_config_toml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".void-config"),
            r#"
[index]
ignore = ["**/target/**", "**/*.pb.rs"]

[audit]
suppress = [
    { rule = "unwrap-*", path = "src/audit/**" },
]

[analysis]
cc_threshold = 20

[diagram]
default_format = "mermaid"
"#,
        )
        .unwrap();

        let cfg = ProjectConfig::load(tmp.path());
        assert_eq!(cfg.index.ignore.len(), 2);
        assert_eq!(cfg.audit.suppress.len(), 1);
        assert_eq!(cfg.audit.suppress[0].rule, "unwrap-*");
        assert_eq!(cfg.analysis.cc_threshold, 20);
        assert_eq!(cfg.diagram.default_format, "mermaid");
    }

    #[test]
    fn test_load_legacy_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".voidignore"),
            "# comment\n**/target/**\n**/*.pb.rs\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".void-audit-ignore"),
            "unwrap-*  src/audit/**\n",
        )
        .unwrap();

        let cfg = ProjectConfig::load(tmp.path());
        assert_eq!(cfg.index.ignore, vec!["**/target/**", "**/*.pb.rs"]);
        assert_eq!(cfg.audit.suppress.len(), 1);
        assert_eq!(cfg.audit.suppress[0].rule, "unwrap-*");
    }

    #[test]
    fn test_load_empty_dir_returns_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = ProjectConfig::load(tmp.path());
        assert!(cfg.index.ignore.is_empty());
        assert!(cfg.audit.suppress.is_empty());
        assert_eq!(cfg.analysis.cc_threshold, 10);
        assert_eq!(cfg.diagram.default_format, "drawio");
    }

    #[test]
    fn test_save_and_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = ProjectConfig::default();
        cfg.index.ignore = vec!["**/build/**".into()];
        cfg.ai.default_model = Some("llama3.2".into());
        cfg.save(tmp.path()).unwrap();

        let loaded = ProjectConfig::load(tmp.path());
        assert_eq!(loaded.index.ignore, vec!["**/build/**"]);
        assert_eq!(loaded.ai.default_model.as_deref(), Some("llama3.2"));

        // Check the file has the header comment
        let content = std::fs::read_to_string(tmp.path().join(".void-config")).unwrap();
        assert!(content.starts_with("# .void-config"));
    }

    #[test]
    fn test_migrate_creates_config_and_backs_up() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".voidignore"), "**/target/**\n").unwrap();
        std::fs::write(tmp.path().join(".void-audit-ignore"), "unwrap-*  src/**\n").unwrap();

        let report = migrate_legacy_config(tmp.path()).unwrap();
        match report {
            MigrationReport::Migrated {
                voidignore_rules,
                audit_suppressions,
            } => {
                assert_eq!(voidignore_rules, 1);
                assert_eq!(audit_suppressions, 1);
            }
            other => panic!("Expected Migrated, got {:?}", other),
        }

        // .void-config now exists
        assert!(tmp.path().join(".void-config").exists());
        // Legacy files backed up
        assert!(tmp.path().join(".voidignore.backup").exists());
        assert!(tmp.path().join(".void-audit-ignore.backup").exists());
        // Originals gone
        assert!(!tmp.path().join(".voidignore").exists());
        assert!(!tmp.path().join(".void-audit-ignore").exists());
    }

    #[test]
    fn test_migrate_already_migrated() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".void-config"), "[index]\n").unwrap();
        let report = migrate_legacy_config(tmp.path()).unwrap();
        assert!(matches!(report, MigrationReport::AlreadyMigrated));
    }

    #[test]
    fn test_migrate_nothing_to_migrate() {
        let tmp = tempfile::tempdir().unwrap();
        let report = migrate_legacy_config(tmp.path()).unwrap();
        assert!(matches!(report, MigrationReport::NothingToMigrate));
    }

    #[test]
    fn test_void_config_takes_precedence_over_legacy() {
        let tmp = tempfile::tempdir().unwrap();
        // Both exist — .void-config should win
        std::fs::write(
            tmp.path().join(".void-config"),
            "[index]\nignore = [\"from-config\"]\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join(".voidignore"), "from-legacy\n").unwrap();

        let cfg = ProjectConfig::load(tmp.path());
        assert_eq!(cfg.index.ignore, vec!["from-config"]);
    }
}
