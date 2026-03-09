//! Python best practices via ruff.

use std::path::Path;

use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project has Python files.
pub fn is_relevant(project_path: &Path) -> bool {
    project_path.join("requirements.txt").exists()
        || project_path.join("pyproject.toml").exists()
        || project_path.join("setup.py").exists()
}

/// Run ruff and parse its JSON output.
pub fn run_ruff(project_path: &Path) -> Vec<BestPracticesFinding> {
    let mut findings = Vec::new();

    let output = run_command_timeout(
        "ruff",
        &["check", ".", "--output-format", "json", "--no-cache"],
        project_path,
        60,
    );

    let output = match output {
        Some(o) => o,
        None => {
            findings.push(BestPracticesFinding {
                rule_id: "ruff-missing".into(),
                tool: "ruff".into(),
                category: BpCategory::Style,
                severity: BpSeverity::Suggestion,
                file: String::new(),
                line: None,
                col: None,
                message: "ruff no instalado — instalar con: pip install ruff".into(),
                fix_hint: Some("pip install ruff".into()),
            });
            return findings;
        }
    };

    let items: Vec<serde_json::Value> = match serde_json::from_str(&output) {
        Ok(v) => v,
        Err(_) => return findings,
    };

    for item in &items {
        let code = item.get("code").and_then(|c| c.as_str()).unwrap_or("unknown");

        // Skip S-prefix rules (security) — covered by Phase 9 audit
        if code.starts_with('S') {
            continue;
        }

        let message = item.get("message").and_then(|m| m.as_str()).unwrap_or("");
        let filename = item.get("filename").and_then(|f| f.as_str()).unwrap_or("");
        let line = item.get("location").and_then(|l| l.get("row")).and_then(|r| r.as_u64()).map(|r| r as usize);
        let col = item.get("location").and_then(|l| l.get("column")).and_then(|c| c.as_u64()).map(|c| c as usize);

        let fix_hint = item.get("fix")
            .and_then(|f| f.get("message"))
            .and_then(|m| m.as_str())
            .map(String::from);

        let severity = map_ruff_severity(code);
        let category = map_ruff_category(code);

        // Make file path relative to project
        let rel_file = if let Some(stripped) = filename.strip_prefix(&project_path.to_string_lossy().as_ref()) {
            stripped.trim_start_matches(['/', '\\']).to_string()
        } else {
            filename.to_string()
        };

        findings.push(BestPracticesFinding {
            rule_id: format!("ruff:{}", code),
            tool: "ruff".into(),
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

fn map_ruff_severity(code: &str) -> BpSeverity {
    let prefix = &code[..1.min(code.len())];
    match prefix {
        "F" => {
            // F8xx (undefined names) are Important
            if code.starts_with("F8") { BpSeverity::Important } else { BpSeverity::Warning }
        }
        "E" => BpSeverity::Warning,
        "B" => {
            if code.starts_with("B0") { BpSeverity::Important } else { BpSeverity::Warning }
        }
        "C" => BpSeverity::Warning,
        "N" | "I" => BpSeverity::Suggestion,
        _ if code.starts_with("UP") => BpSeverity::Suggestion,
        _ if code.starts_with("RUF") => BpSeverity::Warning,
        _ => BpSeverity::Suggestion,
    }
}

fn map_ruff_category(code: &str) -> BpCategory {
    if code.starts_with('F') { return BpCategory::Correctness; }
    if code.starts_with('E') { return BpCategory::Style; }
    if code.starts_with('B') { return BpCategory::Correctness; }
    if code.starts_with('C') { return BpCategory::Complexity; }
    if code.starts_with('N') || code.starts_with('I') { return BpCategory::Style; }
    if code.starts_with("UP") { return BpCategory::Idiom; }
    if code.starts_with("RUF") { return BpCategory::Idiom; }
    BpCategory::Style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ruff_json() {
        let json = r#"[
            {
                "code": "B006",
                "message": "Do not use mutable data structures for argument defaults",
                "filename": "/path/to/file.py",
                "location": { "row": 14, "column": 12 },
                "end_location": { "row": 14, "column": 14 },
                "fix": { "message": "Replace with None; initialize within function" }
            },
            {
                "code": "S101",
                "message": "Use of assert detected",
                "filename": "/path/to/test.py",
                "location": { "row": 5, "column": 1 },
                "end_location": { "row": 5, "column": 7 },
                "fix": null
            }
        ]"#;

        let items: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        assert_eq!(items.len(), 2);

        // S-prefix should be skipped
        let code0 = items[0].get("code").and_then(|c| c.as_str()).unwrap();
        assert!(!code0.starts_with('S'));
        let code1 = items[1].get("code").and_then(|c| c.as_str()).unwrap();
        assert!(code1.starts_with('S')); // this would be filtered
    }

    #[test]
    fn test_severity_mapping() {
        assert_eq!(map_ruff_severity("F841"), BpSeverity::Important);
        assert_eq!(map_ruff_severity("E501"), BpSeverity::Warning);
        assert_eq!(map_ruff_severity("B006"), BpSeverity::Important);
        assert_eq!(map_ruff_severity("N801"), BpSeverity::Suggestion);
        assert_eq!(map_ruff_severity("I001"), BpSeverity::Suggestion);
        assert_eq!(map_ruff_severity("UP035"), BpSeverity::Suggestion);
    }
}
