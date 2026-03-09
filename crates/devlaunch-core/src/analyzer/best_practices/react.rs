//! React / TypeScript best practices via react-doctor.

use std::path::Path;

use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project has React dependencies.
pub fn is_relevant(project_path: &Path) -> bool {
    let pkg = project_path.join("package.json");
    if !pkg.exists() { return false; }
    if let Ok(content) = std::fs::read_to_string(&pkg) {
        content.contains("\"react\"")
    } else {
        false
    }
}

/// Run react-doctor and parse its JSON output.
/// Returns (findings, native_score).
pub fn run_react_doctor(project_path: &Path) -> (Vec<BestPracticesFinding>, Option<f32>) {
    let mut findings = Vec::new();

    let output = run_command_timeout(
        "npx",
        &["-y", "react-doctor", "--json"],
        project_path,
        60,
    );

    let output = match output {
        Some(o) => o,
        None => {
            findings.push(BestPracticesFinding {
                rule_id: "react-doctor-missing".into(),
                tool: "react-doctor".into(),
                category: BpCategory::Style,
                severity: BpSeverity::Suggestion,
                file: String::new(),
                line: None,
                col: None,
                message: "react-doctor no disponible — instalar via: npx -y react-doctor".into(),
                fix_hint: Some("npx -y react-doctor".into()),
            });
            return (findings, None);
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&output) {
        Ok(v) => v,
        Err(_) => return (findings, None),
    };

    // Extract native score
    let native_score = json.get("score")
        .and_then(|s| s.get("score"))
        .and_then(|s| s.as_f64())
        .map(|s| s as f32);

    // Parse diagnostics
    if let Some(diagnostics) = json.get("diagnostics").and_then(|d| d.as_array()) {
        for diag in diagnostics {
            let rule_id = diag.get("ruleId").and_then(|r| r.as_str()).unwrap_or("unknown");
            let severity_str = diag.get("severity").and_then(|s| s.as_str()).unwrap_or("suggestion");
            let message = diag.get("message").and_then(|m| m.as_str()).unwrap_or("");
            let file = diag.get("file").and_then(|f| f.as_str()).unwrap_or("");
            let line = diag.get("line").and_then(|l| l.as_u64()).map(|l| l as usize);
            let col = diag.get("column").and_then(|c| c.as_u64()).map(|c| c as usize);
            let category_str = diag.get("category").and_then(|c| c.as_str()).unwrap_or("");

            let severity = match severity_str {
                "error" => BpSeverity::Important,
                "warning" => BpSeverity::Warning,
                _ => BpSeverity::Suggestion,
            };

            let category = match category_str {
                "performance" => BpCategory::Performance,
                "bundle-size" => BpCategory::BundleSize,
                "state-effects" | "correctness" => BpCategory::Correctness,
                "architecture" => BpCategory::Complexity,
                "dead-code" => BpCategory::DeadCode,
                "accessibility" => BpCategory::Accessibility,
                _ => BpCategory::Style,
            };

            findings.push(BestPracticesFinding {
                rule_id: rule_id.to_string(),
                tool: "react-doctor".into(),
                category,
                severity,
                file: file.to_string(),
                line,
                col,
                message: message.to_string(),
                fix_hint: None,
            });
        }
    }

    (findings, native_score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_react_doctor_json() {
        let json = r#"{
            "score": { "score": 82, "label": "Good" },
            "project": { "framework": "next", "reactVersion": "19.0" },
            "diagnostics": [
                {
                    "ruleId": "react/hooks-exhaustive-deps",
                    "severity": "warning",
                    "message": "React Hook useEffect has missing dependency: 'userId'",
                    "file": "src/components/Profile.tsx",
                    "line": 24,
                    "column": 5,
                    "category": "state-effects"
                },
                {
                    "ruleId": "react/no-danger",
                    "severity": "error",
                    "message": "Using dangerouslySetInnerHTML",
                    "file": "src/components/Render.tsx",
                    "line": 10,
                    "column": 3,
                    "category": "correctness"
                }
            ]
        }"#;

        let v: serde_json::Value = serde_json::from_str(json).unwrap();

        // Parse score
        let native_score = v.get("score").and_then(|s| s.get("score")).and_then(|s| s.as_f64()).map(|s| s as f32);
        assert_eq!(native_score, Some(82.0));

        let diagnostics = v.get("diagnostics").and_then(|d| d.as_array()).unwrap();
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_react_not_relevant_without_package_json() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_relevant(dir.path()));
    }
}
