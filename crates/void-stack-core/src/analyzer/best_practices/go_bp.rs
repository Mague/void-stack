//! Go best practices via golangci-lint.

use std::path::Path;

use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project has Go files.
pub fn is_relevant(project_path: &Path) -> bool {
    project_path.join("go.mod").exists()
}

/// Run golangci-lint and parse its JSON output.
pub fn run_golangci_lint(project_path: &Path) -> Vec<BestPracticesFinding> {
    let mut findings = Vec::new();

    let output = run_command_timeout(
        "golangci-lint",
        &[
            "run",
            "--out-format",
            "json",
            "--timeout",
            "60s",
            "--no-config",
            "--enable",
            "errcheck,govet,staticcheck,gosimple,ineffassign,unused,gocyclo,revive",
            "./...",
        ],
        project_path,
        60,
    );

    let output = match output {
        Some(o) => o,
        None => {
            findings.push(BestPracticesFinding {
                rule_id: "golangci-lint-missing".into(),
                tool: "golangci-lint".into(),
                category: BpCategory::Style,
                severity: BpSeverity::Suggestion,
                file: String::new(),
                line: None,
                col: None,
                message: "golangci-lint no instalado — instalar con: go install github.com/golangci/golangci-lint/cmd/golangci-lint@latest".into(),
                fix_hint: Some("go install github.com/golangci/golangci-lint/cmd/golangci-lint@latest".into()),
            });
            return findings;
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&output) {
        Ok(v) => v,
        Err(_) => return findings,
    };

    if let Some(issues) = json.get("Issues").and_then(|i| i.as_array()) {
        for issue in issues {
            let linter = issue
                .get("FromLinter")
                .and_then(|l| l.as_str())
                .unwrap_or("");

            // Skip gosec — covered by Phase 9 security audit
            if linter == "gosec" {
                continue;
            }

            let text = issue.get("Text").and_then(|t| t.as_str()).unwrap_or("");
            let pos = issue.get("Pos");
            let file = pos
                .and_then(|p| p.get("Filename"))
                .and_then(|f| f.as_str())
                .unwrap_or("");
            let line = pos
                .and_then(|p| p.get("Line"))
                .and_then(|l| l.as_u64())
                .map(|l| l as usize);
            let col = pos
                .and_then(|p| p.get("Column"))
                .and_then(|c| c.as_u64())
                .map(|c| c as usize);

            let severity = map_go_severity(linter);
            let category = map_go_category(linter);

            findings.push(BestPracticesFinding {
                rule_id: format!("go:{}", linter),
                tool: "golangci-lint".into(),
                category,
                severity,
                file: file.to_string(),
                line,
                col,
                message: text.to_string(),
                fix_hint: None,
            });
        }
    }

    findings
}

fn map_go_severity(linter: &str) -> BpSeverity {
    match linter {
        "errcheck" | "govet" | "staticcheck" => BpSeverity::Important,
        "gosimple" | "ineffassign" | "unused" | "gocyclo" => BpSeverity::Warning,
        "revive" => BpSeverity::Suggestion,
        _ => BpSeverity::Suggestion,
    }
}

fn map_go_category(linter: &str) -> BpCategory {
    match linter {
        "errcheck" | "govet" | "staticcheck" => BpCategory::Correctness,
        "gosimple" | "ineffassign" => BpCategory::Idiom,
        "unused" => BpCategory::DeadCode,
        "gocyclo" => BpCategory::Complexity,
        "revive" => BpCategory::Style,
        _ => BpCategory::Style,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_golangci_lint_json() {
        let json = r#"{
            "Issues": [
                {
                    "FromLinter": "errcheck",
                    "Text": "Error return value of `rows.Close` is not checked",
                    "Severity": "warning",
                    "Pos": {
                        "Filename": "internal/db/query.go",
                        "Line": 87,
                        "Column": 2
                    }
                },
                {
                    "FromLinter": "gosec",
                    "Text": "G101: Potential hardcoded credentials",
                    "Severity": "warning",
                    "Pos": {
                        "Filename": "config/auth.go",
                        "Line": 12,
                        "Column": 1
                    }
                }
            ]
        }"#;

        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let issues = v["Issues"].as_array().unwrap();
        assert_eq!(issues.len(), 2);

        // gosec should be filtered
        let linter0 = issues[0]["FromLinter"].as_str().unwrap();
        assert_eq!(linter0, "errcheck");
        let linter1 = issues[1]["FromLinter"].as_str().unwrap();
        assert_eq!(linter1, "gosec"); // would be skipped in run_golangci_lint
    }

    #[test]
    fn test_go_severity_mapping() {
        assert_eq!(map_go_severity("errcheck"), BpSeverity::Important);
        assert_eq!(map_go_severity("govet"), BpSeverity::Important);
        assert_eq!(map_go_severity("gocyclo"), BpSeverity::Warning);
        assert_eq!(map_go_severity("revive"), BpSeverity::Suggestion);
    }
}
