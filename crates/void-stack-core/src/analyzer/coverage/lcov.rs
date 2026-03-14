//! LCOV format parser (Flutter, genhtml, lcov).
//!
//! Format:
//! ```text
//! SF:/path/to/file.dart
//! DA:1,1
//! DA:2,0
//! LH:1
//! LF:2
//! end_of_record
//! ```

use super::{CoverageData, FileCoverage};

pub fn parse(content: &str) -> Option<CoverageData> {
    let mut files = Vec::new();
    let mut current_file: Option<String> = None;
    let mut current_lh: usize = 0; // lines hit
    let mut current_lf: usize = 0; // lines found

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(path) = trimmed.strip_prefix("SF:") {
            current_file = Some(path.to_string());
            current_lh = 0;
            current_lf = 0;
        } else if let Some(rest) = trimmed.strip_prefix("LH:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                current_lh = n;
            }
        } else if let Some(rest) = trimmed.strip_prefix("LF:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                current_lf = n;
            }
        } else if trimmed == "end_of_record" {
            if let Some(path) = current_file.take() {
                let pct = if current_lf > 0 {
                    current_lh as f32 / current_lf as f32 * 100.0
                } else {
                    0.0
                };
                files.push(FileCoverage {
                    path,
                    total_lines: current_lf,
                    covered_lines: current_lh,
                    coverage_percent: pct,
                });
            }
            current_lh = 0;
            current_lf = 0;
        }
    }

    if files.is_empty() {
        return None;
    }

    let mut data = CoverageData {
        tool: "lcov".to_string(),
        total_lines: 0,
        covered_lines: 0,
        coverage_percent: 0.0,
        files,
    };
    data.recalculate();
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lcov() {
        let content = r#"SF:lib/src/models/user.dart
DA:1,1
DA:2,1
DA:3,0
LF:3
LH:2
end_of_record
SF:lib/src/services/auth.dart
DA:1,1
DA:2,1
DA:3,1
DA:4,0
DA:5,0
LF:5
LH:3
end_of_record
"#;
        let data = parse(content).unwrap();
        assert_eq!(data.files.len(), 2);
        assert_eq!(data.total_lines, 8);
        assert_eq!(data.covered_lines, 5);
        assert!((data.coverage_percent - 62.5).abs() < 0.1);

        assert_eq!(data.files[0].path, "lib/src/models/user.dart");
        assert!((data.files[0].coverage_percent - 66.6).abs() < 0.5);
    }

    #[test]
    fn test_parse_lcov_single_file() {
        let content = "SF:src/main.rs\nLF:10\nLH:10\nend_of_record\n";
        let data = parse(content).unwrap();
        assert_eq!(data.files.len(), 1);
        assert!((data.coverage_percent - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_lcov_zero_coverage() {
        let content = "SF:src/app.rs\nLF:20\nLH:0\nend_of_record\n";
        let data = parse(content).unwrap();
        assert_eq!(data.covered_lines, 0);
        assert_eq!(data.coverage_percent, 0.0);
    }

    #[test]
    fn test_parse_lcov_empty() {
        assert!(parse("").is_none());
        assert!(parse("SF:file.rs\n").is_none()); // no end_of_record
    }

    #[test]
    fn test_parse_lcov_tool_name() {
        let content = "SF:a.rs\nLF:1\nLH:1\nend_of_record\n";
        let data = parse(content).unwrap();
        assert_eq!(data.tool, "lcov");
    }
}
