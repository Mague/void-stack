//! Astro best practices via eslint-plugin-astro (ESLint fallback).

use std::path::Path;

use super::vue::parse_eslint_json;
use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project uses Astro.
pub fn is_relevant(project_path: &Path) -> bool {
    // astro.config.mjs/ts is the definitive marker
    if project_path.join("astro.config.mjs").exists()
        || project_path.join("astro.config.ts").exists()
        || project_path.join("astro.config.js").exists()
    {
        return true;
    }
    // Also check package.json
    let pkg = project_path.join("package.json");
    if pkg.exists()
        && let Ok(content) = std::fs::read_to_string(&pkg)
    {
        return content.contains("\"astro\"");
    }
    false
}

/// Run eslint with astro plugin and parse JSON output.
pub fn run_eslint_astro(project_path: &Path) -> Vec<BestPracticesFinding> {
    let mut findings = Vec::new();

    // Try project-local eslint (it may have eslint-plugin-astro configured)
    let output = run_command_timeout(
        "npx",
        &[
            "eslint",
            "--ext",
            ".astro,.js,.ts,.jsx,.tsx",
            "--format",
            "json",
            "--no-error-on-unmatched-pattern",
            ".",
        ],
        project_path,
        90,
    );

    let output = match output {
        Some(o) => o,
        None => {
            findings.push(BestPracticesFinding {
                rule_id: "eslint-astro-missing".into(),
                tool: "eslint-plugin-astro".into(),
                category: BpCategory::Style,
                severity: BpSeverity::Suggestion,
                file: String::new(),
                line: None,
                col: None,
                message: "ESLint con eslint-plugin-astro no disponible — instalar: npm i -D eslint eslint-plugin-astro".into(),
                fix_hint: Some("npm i -D eslint eslint-plugin-astro".into()),
            });
            return findings;
        }
    };

    parse_eslint_json(&output, "eslint-plugin-astro", project_path, &mut findings);

    // Remap Astro-specific rules
    for finding in &mut findings {
        if finding.rule_id.contains("astro/") {
            finding.category = map_astro_category(&finding.rule_id);
        }
    }

    findings
}

fn map_astro_category(rule_id: &str) -> BpCategory {
    if rule_id.contains("no-unused") {
        return BpCategory::DeadCode;
    }
    if rule_id.contains("valid-") || rule_id.contains("no-conflict") {
        return BpCategory::Correctness;
    }
    if rule_id.contains("prefer-") || rule_id.contains("astro/") {
        return BpCategory::Idiom;
    }
    BpCategory::Style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_astro_category_mapping() {
        assert_eq!(
            map_astro_category("eslint:astro/no-unused-define-vars-in-style"),
            BpCategory::DeadCode
        );
        assert_eq!(
            map_astro_category("eslint:astro/valid-compile"),
            BpCategory::Correctness
        );
        assert_eq!(
            map_astro_category("eslint:astro/prefer-class-list-directive"),
            BpCategory::Idiom
        );
    }

    #[test]
    fn test_astro_not_relevant_without_markers() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_relevant(dir.path()));
    }
}
