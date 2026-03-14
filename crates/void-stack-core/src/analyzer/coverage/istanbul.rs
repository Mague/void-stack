//! Istanbul/c8 JSON coverage parser (Node.js).
//!
//! Supports two formats:
//! - coverage-summary.json: { "total": { "lines": { "total": N, "covered": N } }, "/path": { ... } }
//! - coverage-final.json: { "/path": { "s": { "0": 1, "1": 0 }, ... } }

use super::{CoverageData, FileCoverage};

pub fn parse(content: &str) -> Option<CoverageData> {
    let trimmed = content.trim();
    if !trimmed.starts_with('{') {
        return None;
    }

    // Try coverage-summary.json format first
    if let Some(data) = parse_summary(content) {
        return Some(data);
    }

    // Try coverage-final.json format
    parse_final(content)
}

/// Parse coverage-summary.json format.
fn parse_summary(content: &str) -> Option<CoverageData> {
    // Look for "total" key with "lines" sub-object
    if !content.contains("\"total\"") || !content.contains("\"lines\"") {
        return None;
    }

    let mut files = Vec::new();

    // Extract file entries - each top-level key (except "total") is a file
    // Simple state machine to find top-level keys
    let mut depth = 0;
    let mut current_key = String::new();
    let mut brace_start = 0;
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '"' if depth == 1 => {
                current_key.clear();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    current_key.push(chars[i]);
                    i += 1;
                }
            }
            '{' => {
                if depth == 1 {
                    brace_start = i;
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 1 && !current_key.is_empty() {
                    let value_str = &content[brace_start..=i];
                    if let Some(fc) = extract_summary_file(&current_key, value_str)
                        && current_key != "total"
                    {
                        files.push(fc);
                    }
                    current_key.clear();
                }
            }
            _ => {}
        }
        i += 1;
    }

    if files.is_empty() {
        return None;
    }

    let mut data = CoverageData {
        tool: "istanbul".to_string(),
        total_lines: 0,
        covered_lines: 0,
        coverage_percent: 0.0,
        files,
    };
    data.recalculate();
    Some(data)
}

fn extract_summary_file(path: &str, value: &str) -> Option<FileCoverage> {
    // Look for "lines": { "total": N, "covered": N, "pct": N }
    let lines_pos = value.find("\"lines\"")?;
    let after = &value[lines_pos..];

    let total = extract_nested_number(after, "total")?;
    let covered = extract_nested_number(after, "covered")?;
    let pct = extract_nested_float(after, "pct").unwrap_or(if total > 0 {
        covered as f32 / total as f32 * 100.0
    } else {
        0.0
    });

    Some(FileCoverage {
        path: path.to_string(),
        total_lines: total,
        covered_lines: covered,
        coverage_percent: pct,
    })
}

/// Parse coverage-final.json format (statement-level coverage).
fn parse_final(content: &str) -> Option<CoverageData> {
    if !content.contains("\"s\"") {
        return None;
    }

    let mut files = Vec::new();

    // Find file entries: each top-level key is a file path
    // Extract statement map counts from "s": { "0": N, "1": N, ... }
    let mut depth = 0;
    let mut current_key = String::new();
    let mut brace_start = 0;
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '"' if depth == 1 => {
                current_key.clear();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    current_key.push(chars[i]);
                    i += 1;
                }
            }
            '{' => {
                if depth == 1 {
                    brace_start = i;
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 1 && !current_key.is_empty() {
                    let value_str = &content[brace_start..=i];
                    if let Some(fc) = extract_final_file(&current_key, value_str) {
                        files.push(fc);
                    }
                    current_key.clear();
                }
            }
            _ => {}
        }
        i += 1;
    }

    if files.is_empty() {
        return None;
    }

    let mut data = CoverageData {
        tool: "istanbul".to_string(),
        total_lines: 0,
        covered_lines: 0,
        coverage_percent: 0.0,
        files,
    };
    data.recalculate();
    Some(data)
}

fn extract_final_file(path: &str, value: &str) -> Option<FileCoverage> {
    // Count statements: "s": { "0": 1, "1": 0, "2": 3, ... }
    let s_pos = value.find("\"s\"")?;
    let after = &value[s_pos..];
    let brace = after.find('{')?;
    let end_brace = after[brace..].find('}')?;
    let stmts = &after[brace + 1..brace + end_brace];

    let mut total = 0usize;
    let mut covered = 0usize;

    for part in stmts.split(',') {
        if let Some(colon) = part.find(':') {
            let val_str = part[colon + 1..].trim();
            if let Ok(hits) = val_str.parse::<u32>() {
                total += 1;
                if hits > 0 {
                    covered += 1;
                }
            }
        }
    }

    if total == 0 {
        return None;
    }

    Some(FileCoverage {
        path: path.to_string(),
        total_lines: total,
        covered_lines: covered,
        coverage_percent: covered as f32 / total as f32 * 100.0,
    })
}

fn extract_nested_number(content: &str, key: &str) -> Option<usize> {
    let pattern = format!("\"{}\"", key);
    let pos = content.find(&pattern)? + pattern.len();
    let after = &content[pos..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim();
    let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

fn extract_nested_float(content: &str, key: &str) -> Option<f32> {
    let pattern = format!("\"{}\"", key);
    let pos = content.find(&pattern)? + pattern.len();
    let after = &content[pos..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim();
    let num_str: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    num_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_summary() {
        let content = r#"{
  "total": { "lines": { "total": 100, "covered": 75, "skipped": 0, "pct": 75 } },
  "src/app.ts": { "lines": { "total": 60, "covered": 50, "skipped": 0, "pct": 83.33 } },
  "src/utils.ts": { "lines": { "total": 40, "covered": 25, "skipped": 0, "pct": 62.5 } }
}"#;
        let data = parse(content).unwrap();
        assert_eq!(data.files.len(), 2);
        assert_eq!(data.total_lines, 100);
        assert_eq!(data.covered_lines, 75);
    }

    #[test]
    fn test_parse_final_format() {
        let content = r#"{
  "/src/app.ts": { "s": { "0": 1, "1": 1, "2": 0, "3": 1 } },
  "/src/utils.ts": { "s": { "0": 1, "1": 0 } }
}"#;
        let data = parse(content).unwrap();
        assert_eq!(data.files.len(), 2);
        assert_eq!(data.tool, "istanbul");
    }

    #[test]
    fn test_parse_invalid_json() {
        assert!(parse("not json").is_none());
        assert!(parse("[]").is_none());
    }

    #[test]
    fn test_parse_empty_object() {
        assert!(parse("{}").is_none());
    }

    #[test]
    fn test_extract_nested_number_fn() {
        let json = r#"{ "total": 42, "covered": 30 }"#;
        assert_eq!(extract_nested_number(json, "total"), Some(42));
        assert_eq!(extract_nested_number(json, "covered"), Some(30));
        assert_eq!(extract_nested_number(json, "missing"), None);
    }

    #[test]
    fn test_extract_nested_float_fn() {
        let json = r#"{ "pct": 83.33 }"#;
        let pct = extract_nested_float(json, "pct").unwrap();
        assert!((pct - 83.33).abs() < 0.01);
    }

    #[test]
    fn test_summary_single_file() {
        let content = r#"{
  "total": { "lines": { "total": 50, "covered": 40, "pct": 80 } },
  "index.js": { "lines": { "total": 50, "covered": 40, "pct": 80 } }
}"#;
        let data = parse(content).unwrap();
        assert_eq!(data.files.len(), 1);
        assert_eq!(data.files[0].path, "index.js");
    }
}
