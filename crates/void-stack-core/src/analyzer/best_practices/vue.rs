//! Vue.js best practices via eslint-plugin-vue (ESLint fallback).
//! Only runs if Oxlint didn't produce results for Vue files.

use std::path::Path;

use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project uses Vue.
pub fn is_relevant(project_path: &Path) -> bool {
    let pkg = project_path.join("package.json");
    if !pkg.exists() {
        return false;
    }
    if let Ok(content) = std::fs::read_to_string(&pkg) {
        content.contains("\"vue\"") || content.contains("\"nuxt\"")
    } else {
        false
    }
}

/// Run eslint with vue plugin and parse JSON output.
pub fn run_eslint_vue(project_path: &Path) -> Vec<BestPracticesFinding> {
    let mut findings = Vec::new();

    // Try project-local eslint first (it may have eslint-plugin-vue configured)
    let output = run_command_timeout(
        "npx",
        &[
            "eslint",
            "--ext",
            ".vue,.js,.ts",
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
                rule_id: "eslint-vue-missing".into(),
                tool: "eslint-plugin-vue".into(),
                category: BpCategory::Style,
                severity: BpSeverity::Suggestion,
                file: String::new(),
                line: None,
                col: None,
                message: "ESLint con eslint-plugin-vue no disponible — instalar: npm i -D eslint eslint-plugin-vue".into(),
                fix_hint: Some("npm i -D eslint eslint-plugin-vue".into()),
            });
            return findings;
        }
    };

    parse_eslint_json(&output, "eslint-plugin-vue", project_path, &mut findings);
    findings
}

/// Parse ESLint JSON output format (shared between Vue/Angular/Astro).
pub(crate) fn parse_eslint_json(
    output: &str,
    tool_name: &str,
    project_path: &Path,
    findings: &mut Vec<BestPracticesFinding>,
) {
    let files: Vec<serde_json::Value> = match serde_json::from_str(output) {
        Ok(v) => v,
        Err(_) => return,
    };

    for file_entry in &files {
        let filepath = file_entry
            .get("filePath")
            .and_then(|f| f.as_str())
            .unwrap_or("");
        let messages = match file_entry.get("messages").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => continue,
        };

        // Make path relative
        let rel_file = if let Some(stripped) =
            filepath.strip_prefix(project_path.to_string_lossy().as_ref())
        {
            stripped.trim_start_matches(['/', '\\']).to_string()
        } else {
            filepath.to_string()
        };

        for msg in messages {
            let rule_id = msg
                .get("ruleId")
                .and_then(|r| r.as_str())
                .unwrap_or("unknown");
            let severity_num = msg.get("severity").and_then(|s| s.as_u64()).unwrap_or(1);
            let message = msg.get("message").and_then(|m| m.as_str()).unwrap_or("");
            let line = msg.get("line").and_then(|l| l.as_u64()).map(|l| l as usize);
            let col = msg
                .get("column")
                .and_then(|c| c.as_u64())
                .map(|c| c as usize);

            let severity = match severity_num {
                2 => BpSeverity::Important,
                1 => BpSeverity::Warning,
                _ => BpSeverity::Suggestion,
            };

            let category = map_eslint_vue_category(rule_id);

            findings.push(BestPracticesFinding {
                rule_id: format!("eslint:{}", rule_id),
                tool: tool_name.into(),
                category,
                severity,
                file: rel_file.clone(),
                line,
                col,
                message: message.to_string(),
                fix_hint: None,
            });
        }
    }
}

fn map_eslint_vue_category(rule_id: &str) -> BpCategory {
    if rule_id.starts_with("vue/no-unused") || rule_id.contains("dead") {
        return BpCategory::DeadCode;
    }
    if rule_id.contains("complexity") {
        return BpCategory::Complexity;
    }
    if rule_id.starts_with("vue/require-") || rule_id.starts_with("vue/valid-") {
        return BpCategory::Correctness;
    }
    if rule_id.starts_with("vue/") {
        return BpCategory::Idiom;
    }
    BpCategory::Style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_eslint_json_output() {
        let json = r#"[
            {
                "filePath": "/project/src/App.vue",
                "messages": [
                    {
                        "ruleId": "vue/no-unused-vars",
                        "severity": 1,
                        "message": "'count' is defined but never used",
                        "line": 15,
                        "column": 7
                    },
                    {
                        "ruleId": "vue/require-default-prop",
                        "severity": 2,
                        "message": "Prop 'title' requires a default value",
                        "line": 8,
                        "column": 3
                    }
                ],
                "errorCount": 1,
                "warningCount": 1
            }
        ]"#;

        let mut findings = Vec::new();
        parse_eslint_json(
            json,
            "eslint-plugin-vue",
            Path::new("/project"),
            &mut findings,
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].severity, BpSeverity::Warning);
        assert_eq!(findings[0].category, BpCategory::DeadCode);
        assert_eq!(findings[1].severity, BpSeverity::Important);
        assert_eq!(findings[1].category, BpCategory::Correctness);
    }

    #[test]
    fn test_vue_not_relevant_without_package_json() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_relevant(dir.path()));
    }
}
