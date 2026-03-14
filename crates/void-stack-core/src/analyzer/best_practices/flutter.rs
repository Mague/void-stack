//! Flutter / Dart best practices via dart analyze / flutter analyze.

use std::path::Path;

use super::{BestPracticesFinding, BpCategory, BpSeverity, run_command_timeout};

/// Check if the project has pubspec.yaml and dart files.
pub fn is_relevant(project_path: &Path) -> bool {
    project_path.join("pubspec.yaml").exists() && project_path.join("lib").is_dir()
}

/// Run flutter analyze (or dart analyze) and parse --machine output.
pub fn run_dart_analyze(project_path: &Path) -> Vec<BestPracticesFinding> {
    let mut findings = Vec::new();

    // Try flutter first, fallback to dart
    let output = run_command_timeout(
        "flutter",
        &["analyze", "--no-pub", "--machine"],
        project_path,
        60,
    )
    .or_else(|| {
        run_command_timeout(
            "dart",
            &["analyze", "--format", "machine", "."],
            project_path,
            60,
        )
    });

    let output = match output {
        Some(o) => o,
        None => {
            findings.push(BestPracticesFinding {
                rule_id: "dart-analyze-missing".into(),
                tool: "dart-analyze".into(),
                category: BpCategory::Style,
                severity: BpSeverity::Suggestion,
                file: String::new(),
                line: None,
                col: None,
                message:
                    "flutter/dart analyze no disponible — instalar Flutter SDK desde flutter.dev"
                        .into(),
                fix_hint: Some("Instalar Flutter SDK desde https://flutter.dev".into()),
            });
            return findings;
        }
    };

    // --machine format: SEVERITY|TYPE|CODE|FILE|LINE|COL|LENGTH|MESSAGE
    for line in output.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 8 {
            continue;
        }

        let severity_str = parts[0];
        let code = parts[2];
        let file = parts[3];
        let line_num = parts[4].parse::<usize>().ok();
        let col_num = parts[5].parse::<usize>().ok();
        let message = parts[7];

        let severity = match severity_str {
            "ERROR" => BpSeverity::Important,
            "WARNING" => BpSeverity::Warning,
            _ => BpSeverity::Suggestion,
        };

        let category = map_dart_category(code);

        // Make file path relative
        let rel_file =
            if let Some(stripped) = file.strip_prefix(project_path.to_string_lossy().as_ref()) {
                stripped.trim_start_matches(['/', '\\']).to_string()
            } else {
                file.to_string()
            };

        findings.push(BestPracticesFinding {
            rule_id: format!("dart:{}", code),
            tool: "dart-analyze".into(),
            category,
            severity,
            file: rel_file,
            line: line_num,
            col: col_num,
            message: message.to_string(),
            fix_hint: None,
        });
    }

    findings
}

fn map_dart_category(code: &str) -> BpCategory {
    match code {
        "prefer_const_constructors"
        | "avoid_unnecessary_containers"
        | "sized_box_for_whitespace" => BpCategory::Performance,
        "cancel_subscriptions" | "close_sinks" | "avoid_returning_null_for_future" => {
            BpCategory::Correctness
        }
        "dead_code" | "unused_import" | "unused_local_variable" => BpCategory::DeadCode,
        _ => BpCategory::Style,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_machine_format() {
        let line = "INFO|LINT|avoid_print|/path/to/file.dart|42|5|14|Avoid print calls in production code.";
        let parts: Vec<&str> = line.split('|').collect();
        assert_eq!(parts.len(), 8);
        assert_eq!(parts[0], "INFO");
        assert_eq!(parts[2], "avoid_print");
        assert_eq!(parts[4], "42");
        assert_eq!(parts[7], "Avoid print calls in production code.");
    }

    #[test]
    fn test_severity_mapping() {
        let cases = vec![
            ("ERROR|LINT|code|f|1|1|1|msg", BpSeverity::Important),
            ("WARNING|LINT|code|f|1|1|1|msg", BpSeverity::Warning),
            ("INFO|LINT|code|f|1|1|1|msg", BpSeverity::Suggestion),
        ];
        for (line, expected) in cases {
            let parts: Vec<&str> = line.split('|').collect();
            let severity = match parts[0] {
                "ERROR" => BpSeverity::Important,
                "WARNING" => BpSeverity::Warning,
                _ => BpSeverity::Suggestion,
            };
            assert_eq!(severity, expected);
        }
    }
}
