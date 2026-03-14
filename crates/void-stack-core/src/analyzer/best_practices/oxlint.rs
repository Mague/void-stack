//! Frontend best practices via Oxlint (Rust-native linter).
//! Covers React, Vue, Astro, and Svelte with zero config.
//! Used as primary linter in the hybrid strategy (Oxlint first, ESLint fallback).

use std::path::Path;

use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project has any frontend framework files.
pub fn is_relevant(project_path: &Path) -> bool {
    let pkg = project_path.join("package.json");
    if pkg.exists() {
        return true;
    }
    // Check for framework-specific files without package.json
    has_framework_files(project_path)
}

fn has_framework_files(path: &Path) -> bool {
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.ends_with(".astro")
            || name_str.ends_with(".vue")
            || name_str.ends_with(".svelte")
            || name_str.ends_with(".jsx")
            || name_str.ends_with(".tsx")
        {
            return true;
        }
        // Check src/ directory
        if name_str == "src" && entry.path().is_dir() && has_framework_files(&entry.path()) {
            return true;
        }
    }
    false
}

/// Detect which framework the project uses for plugin selection.
fn detect_framework(project_path: &Path) -> Vec<&'static str> {
    let mut plugins = Vec::new();
    let pkg = project_path.join("package.json");

    if let Ok(content) = std::fs::read_to_string(&pkg) {
        if content.contains("\"react\"")
            || content.contains("\"next\"")
            || content.contains("\"preact\"")
        {
            plugins.push("react");
            plugins.push("jsx-a11y");
        }
        if content.contains("\"vue\"") || content.contains("\"nuxt\"") {
            plugins.push("vue");
        }
        if content.contains("\"@angular/core\"") {
            plugins.push("angular");
        }
    }

    // Always include import plugin for better analysis
    plugins.push("import");
    plugins
}

/// Run oxlint and parse its output.
pub fn run_oxlint(project_path: &Path) -> Vec<BestPracticesFinding> {
    let mut findings = Vec::new();

    // Build args with detected plugins
    let plugins = detect_framework(project_path);
    let mut args: Vec<&str> = vec!["--format", "json"];
    for plugin in &plugins {
        args.push("--plugin");
        args.push(plugin);
    }

    let output = run_command_timeout("oxlint", &args, project_path, 60);

    let output = match output {
        Some(o) => o,
        None => return findings, // Oxlint not installed — silent, ESLint fallback will handle it
    };

    // Oxlint JSON output is an array of diagnostics
    let items: Vec<serde_json::Value> = match serde_json::from_str(&output) {
        Ok(v) => v,
        Err(_) => {
            // Try parsing as newline-delimited JSON (oxlint can output this way)
            let mut items = Vec::new();
            for line in output.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    items.push(v);
                }
            }
            if items.is_empty() {
                return findings;
            }
            items
        }
    };

    for item in &items {
        let rule_id = item
            .get("ruleId")
            .or_else(|| item.get("rule_id"))
            .and_then(|r| r.as_str())
            .unwrap_or("unknown");

        let severity_str = item
            .get("severity")
            .and_then(|s| s.as_str())
            .unwrap_or("warning");

        let message = item.get("message").and_then(|m| m.as_str()).unwrap_or("");

        let filename = item
            .get("filename")
            .or_else(|| item.get("file"))
            .and_then(|f| f.as_str())
            .unwrap_or("");

        let line = item
            .get("line")
            .or_else(|| item.get("start").and_then(|s| s.get("line")))
            .and_then(|l| l.as_u64())
            .map(|l| l as usize);

        let col = item
            .get("column")
            .or_else(|| item.get("start").and_then(|s| s.get("column")))
            .and_then(|c| c.as_u64())
            .map(|c| c as usize);

        let fix_hint = item
            .get("fix")
            .and_then(|f| f.get("message").or_else(|| f.get("description")))
            .and_then(|m| m.as_str())
            .map(String::from);

        let severity = match severity_str {
            "error" | "deny" => BpSeverity::Important,
            "warning" | "warn" => BpSeverity::Warning,
            _ => BpSeverity::Suggestion,
        };

        let category = map_oxlint_category(rule_id);

        // Make file path relative
        let rel_file = if let Some(stripped) =
            filename.strip_prefix(project_path.to_string_lossy().as_ref())
        {
            stripped.trim_start_matches(['/', '\\']).to_string()
        } else {
            filename.to_string()
        };

        findings.push(BestPracticesFinding {
            rule_id: format!("oxlint:{}", rule_id),
            tool: "oxlint".into(),
            category,
            severity,
            file: rel_file,
            line,
            col,
            message: message.to_string(),
            fix_hint,
        });
    }

    findings
}

fn map_oxlint_category(rule_id: &str) -> BpCategory {
    if rule_id.contains("a11y") || rule_id.contains("accessibility") {
        return BpCategory::Accessibility;
    }
    if rule_id.contains("perf") || rule_id.contains("exhaustive-deps") {
        return BpCategory::Performance;
    }
    if rule_id.contains("no-unused") || rule_id.contains("dead") {
        return BpCategory::DeadCode;
    }
    if rule_id.contains("import") || rule_id.contains("no-duplicates") {
        return BpCategory::BundleSize;
    }
    if rule_id.contains("complexity") || rule_id.contains("max-") {
        return BpCategory::Complexity;
    }
    if rule_id.contains("no-")
        && (rule_id.contains("unsafe") || rule_id.contains("danger") || rule_id.contains("eval"))
    {
        return BpCategory::Correctness;
    }
    if rule_id.starts_with("react/")
        || rule_id.starts_with("vue/")
        || rule_id.starts_with("angular/")
    {
        return BpCategory::Idiom;
    }
    BpCategory::Style
}

/// Check if oxlint is available on the system.
pub fn is_available() -> bool {
    run_command_timeout("oxlint", &["--version"], Path::new("."), 5).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_mapping() {
        assert_eq!(
            map_oxlint_category("jsx-a11y/alt-text"),
            BpCategory::Accessibility
        );
        assert_eq!(
            map_oxlint_category("react/exhaustive-deps"),
            BpCategory::Performance
        );
        assert_eq!(map_oxlint_category("no-unused-vars"), BpCategory::DeadCode);
        assert_eq!(
            map_oxlint_category("import/no-duplicates"),
            BpCategory::BundleSize
        );
        assert_eq!(
            map_oxlint_category("react/no-danger"),
            BpCategory::Correctness
        );
        assert_eq!(map_oxlint_category("vue/component-name"), BpCategory::Idiom);
    }

    #[test]
    fn test_not_relevant_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_relevant(dir.path()));
    }
}
