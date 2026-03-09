//! Rust best practices via cargo clippy.

use std::path::Path;

use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project has Cargo.toml.
pub fn is_relevant(project_path: &Path) -> bool {
    project_path.join("Cargo.toml").exists()
}

/// Run cargo clippy and parse its JSON output.
pub fn run_clippy(project_path: &Path) -> Vec<BestPracticesFinding> {
    let mut findings = Vec::new();

    // First, quick check that clippy is installed (< 3s)
    let version_check = run_command_timeout(
        "cargo",
        &["clippy", "--version"],
        project_path,
        5,
    );

    if version_check.is_none() {
        findings.push(BestPracticesFinding {
            rule_id: "clippy-missing".into(),
            tool: "clippy".into(),
            category: BpCategory::Style,
            severity: BpSeverity::Suggestion,
            file: String::new(),
            line: None,
            col: None,
            message: "cargo clippy no disponible — instalar con: rustup component add clippy".into(),
            fix_hint: Some("rustup component add clippy".into()),
        });
        return findings;
    }

    // Run actual analysis — 180s timeout (clippy compiles in debug mode on first run)
    let timeout_secs = 180;
    let output = run_command_timeout(
        "cargo",
        &[
            "clippy", "--message-format=json", "--no-deps",
            "--", "-W", "clippy::perf",
            "-W", "clippy::complexity", "-A", "clippy::module_name_repetitions",
        ],
        project_path,
        timeout_secs,
    );

    let output = match output {
        Some(o) => o,
        None => {
            findings.push(BestPracticesFinding {
                rule_id: "clippy-timeout".into(),
                tool: "clippy".into(),
                category: BpCategory::Style,
                severity: BpSeverity::Suggestion,
                file: String::new(),
                line: None,
                col: None,
                message: format!("cargo clippy excedió el tiempo límite ({}s) — proyecto muy grande o necesita compilación", timeout_secs),
                fix_hint: Some("Ejecutar 'cargo build' primero para compilar, luego reintentar".into()),
            });
            return findings;
        }
    };

    // Clippy outputs one JSON object per line
    for line in output.lines() {
        let json: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Only process compiler-message entries
        if json.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }

        let msg = match json.get("message") {
            Some(m) => m,
            None => continue,
        };

        let level = msg.get("level").and_then(|l| l.as_str()).unwrap_or("");
        if level != "warning" && level != "error" {
            continue;
        }

        let code = msg.get("code")
            .and_then(|c| c.get("code"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        // Skip non-clippy warnings (e.g., rustc warnings)
        if code.is_empty() {
            continue;
        }

        let message_text = msg.get("rendered")
            .and_then(|r| r.as_str())
            .unwrap_or_else(|| msg.get("message").and_then(|m| m.as_str()).unwrap_or(""));

        // Extract first line of rendered message for conciseness
        let message_short = message_text.lines().next().unwrap_or(message_text);

        let spans = msg.get("spans").and_then(|s| s.as_array());
        let (file, line_num, col_num) = if let Some(span) = spans.and_then(|s| s.first()) {
            let f = span.get("file_name").and_then(|f| f.as_str()).unwrap_or("");
            let l = span.get("line_start").and_then(|l| l.as_u64()).map(|l| l as usize);
            let c = span.get("column_start").and_then(|c| c.as_u64()).map(|c| c as usize);
            (f.to_string(), l, c)
        } else {
            (String::new(), None, None)
        };

        let fix_hint = msg.get("children")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|child| child.get("message"))
            .and_then(|m| m.as_str())
            .map(String::from);

        let severity = map_clippy_severity(level, code);
        let category = map_clippy_category(code);

        findings.push(BestPracticesFinding {
            rule_id: code.to_string(),
            tool: "clippy".into(),
            category,
            severity,
            file,
            line: line_num,
            col: col_num,
            message: message_short.to_string(),
            fix_hint,
        });
    }

    findings
}

fn map_clippy_severity(level: &str, code: &str) -> BpSeverity {
    if level == "error" {
        return BpSeverity::Important;
    }
    // Group by clippy lint category
    if code.contains("::perf") || code.contains("correctness") || code.contains("suspicious") {
        BpSeverity::Warning
    } else if code.contains("::style") || code.contains("::complexity") || code.contains("::pedantic") {
        BpSeverity::Suggestion
    } else {
        // Default for unknown clippy lints
        BpSeverity::Warning
    }
}

fn map_clippy_category(code: &str) -> BpCategory {
    if code.contains("::perf") { return BpCategory::Performance; }
    if code.contains("::correctness") || code.contains("::suspicious") { return BpCategory::Correctness; }
    if code.contains("::complexity") { return BpCategory::Complexity; }
    if code.contains("::style") || code.contains("::pedantic") { return BpCategory::Style; }
    BpCategory::Idiom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clippy_json_line() {
        let line = r#"{"reason":"compiler-message","message":{"rendered":"warning: called `.unwrap()` on a `Result`","level":"warning","code":{"code":"clippy::unwrap_used"},"spans":[{"file_name":"src/main.rs","line_start":42,"column_start":15}],"children":[{"message":"use `?` operator or `.expect()` with a descriptive message"}]}}"#;

        let json: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(json["reason"].as_str(), Some("compiler-message"));

        let msg = &json["message"];
        let code = msg["code"]["code"].as_str().unwrap();
        assert_eq!(code, "clippy::unwrap_used");

        let span = msg["spans"].as_array().unwrap().first().unwrap();
        assert_eq!(span["file_name"].as_str(), Some("src/main.rs"));
        assert_eq!(span["line_start"].as_u64(), Some(42));
    }

    #[test]
    fn test_filter_non_compiler_messages() {
        let lines = vec![
            r#"{"reason":"build-script-executed","package_id":"void-stack-core 0.1.0"}"#,
            r#"{"reason":"compiler-artifact","package_id":"serde 1.0"}"#,
        ];
        for line in lines {
            let json: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_ne!(json["reason"].as_str(), Some("compiler-message"));
        }
    }
}
