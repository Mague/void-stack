//! Angular best practices via angular-eslint (ng lint) with ESLint fallback.

use std::path::Path;

use super::vue::parse_eslint_json;
use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project uses Angular.
pub fn is_relevant(project_path: &Path) -> bool {
    // angular.json is the definitive marker
    if project_path.join("angular.json").exists() {
        return true;
    }
    // Also check package.json for @angular/core
    let pkg = project_path.join("package.json");
    if pkg.exists()
        && let Ok(content) = std::fs::read_to_string(&pkg)
    {
        return content.contains("\"@angular/core\"");
    }
    false
}

/// Run ng lint (angular-eslint) and parse JSON output.
pub fn run_ng_lint(project_path: &Path) -> Vec<BestPracticesFinding> {
    let mut findings = Vec::new();

    // Try ng lint with JSON format first
    let output = run_command_timeout(
        "npx",
        &["ng", "lint", "--format", "json"],
        project_path,
        120,
    );

    let output = match output {
        Some(o) => o,
        None => {
            // Fallback: try npx eslint directly
            let fallback = run_command_timeout(
                "npx",
                &[
                    "eslint",
                    "--ext",
                    ".ts,.html",
                    "--format",
                    "json",
                    "--no-error-on-unmatched-pattern",
                    "src/",
                ],
                project_path,
                90,
            );
            match fallback {
                Some(o) => o,
                None => {
                    findings.push(BestPracticesFinding {
                        rule_id: "angular-eslint-missing".into(),
                        tool: "angular-eslint".into(),
                        category: BpCategory::Style,
                        severity: BpSeverity::Suggestion,
                        file: String::new(),
                        line: None,
                        col: None,
                        message: "angular-eslint no disponible — instalar: ng add @angular-eslint/schematics".into(),
                        fix_hint: Some("ng add @angular-eslint/schematics".into()),
                    });
                    return findings;
                }
            }
        }
    };

    // ng lint --format json outputs ESLint JSON format
    parse_eslint_json(&output, "angular-eslint", project_path, &mut findings);

    // Remap Angular-specific rules to better categories
    for finding in &mut findings {
        finding.category = map_angular_category(&finding.rule_id);
    }

    findings
}

fn map_angular_category(rule_id: &str) -> BpCategory {
    if rule_id.contains("no-unused") || rule_id.contains("dead") {
        return BpCategory::DeadCode;
    }
    if rule_id.contains("complexity") || rule_id.contains("max-") {
        return BpCategory::Complexity;
    }
    if rule_id.contains("lifecycle") || rule_id.contains("no-empty-lifecycle") {
        return BpCategory::Correctness;
    }
    if rule_id.contains("template-accessibility") || rule_id.contains("a11y") {
        return BpCategory::Accessibility;
    }
    // Performance checks before Idiom (prefer-on-push contains "component-")
    if rule_id.contains("performance") || rule_id.contains("prefer-on-push") {
        return BpCategory::Performance;
    }
    if rule_id.contains("component-") || rule_id.contains("directive-") || rule_id.contains("pipe-")
    {
        return BpCategory::Idiom;
    }
    BpCategory::Style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_angular_category_mapping() {
        assert_eq!(
            map_angular_category("eslint:@angular-eslint/no-empty-lifecycle-method"),
            BpCategory::Correctness
        );
        assert_eq!(
            map_angular_category("eslint:@angular-eslint/component-selector"),
            BpCategory::Idiom
        );
        assert_eq!(
            map_angular_category("eslint:@angular-eslint/template-accessibility-alt-text"),
            BpCategory::Accessibility
        );
        assert_eq!(
            map_angular_category(
                "eslint:@angular-eslint/prefer-on-push-component-change-detection"
            ),
            BpCategory::Performance
        );
        assert_eq!(
            map_angular_category("eslint:no-unused-vars"),
            BpCategory::DeadCode
        );
    }

    #[test]
    fn test_angular_not_relevant_without_markers() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_relevant(dir.path()));
    }
}
