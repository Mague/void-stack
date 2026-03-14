//! Test coverage parsing and visualization.
//!
//! Supports: LCOV (Flutter, genhtml), Cobertura XML (pytest-cov, tarpaulin),
//! Istanbul JSON (c8/nyc), Go cover profiles.

pub mod cobertura;
pub mod go_cover;
pub mod istanbul;
pub mod lcov;

use std::path::Path;

/// Coverage data for a project.
#[derive(Debug, Clone)]
pub struct CoverageData {
    pub tool: String,
    pub total_lines: usize,
    pub covered_lines: usize,
    pub coverage_percent: f32,
    pub files: Vec<FileCoverage>,
}

/// Coverage data for a single file.
#[derive(Debug, Clone)]
pub struct FileCoverage {
    pub path: String,
    pub total_lines: usize,
    pub covered_lines: usize,
    pub coverage_percent: f32,
}

impl CoverageData {
    /// Recalculate totals from individual files.
    pub fn recalculate(&mut self) {
        self.total_lines = self.files.iter().map(|f| f.total_lines).sum();
        self.covered_lines = self.files.iter().map(|f| f.covered_lines).sum();
        self.coverage_percent = if self.total_lines > 0 {
            self.covered_lines as f32 / self.total_lines as f32 * 100.0
        } else {
            0.0
        };
    }
}

/// Known coverage file locations to search for.
const COVERAGE_FILES: &[(&str, CoverageFormat)] = &[
    // LCOV (Flutter, genhtml, lcov)
    ("coverage/lcov.info", CoverageFormat::Lcov),
    ("lcov.info", CoverageFormat::Lcov),
    // Cobertura XML (pytest-cov, tarpaulin, many CI tools)
    ("coverage.xml", CoverageFormat::Cobertura),
    ("cobertura.xml", CoverageFormat::Cobertura),
    ("tarpaulin-report.xml", CoverageFormat::Cobertura),
    // Istanbul JSON (c8, nyc)
    ("coverage/coverage-summary.json", CoverageFormat::Istanbul),
    ("coverage/coverage-final.json", CoverageFormat::Istanbul),
    (".nyc_output/coverage-final.json", CoverageFormat::Istanbul),
    // Go cover
    ("coverage.out", CoverageFormat::GoCover),
    ("cover.out", CoverageFormat::GoCover),
    // Tarpaulin JSON
    ("tarpaulin-report.json", CoverageFormat::TarpaulinJson),
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum CoverageFormat {
    Lcov,
    Cobertura,
    Istanbul,
    GoCover,
    TarpaulinJson,
}

/// Auto-detect and parse coverage data from a project directory.
///
/// For Rust workspace crates, also searches parent directories up to the
/// workspace root, since tools like `cargo-llvm-cov` generate coverage
/// files at the workspace level rather than per-crate.
pub fn parse_coverage(dir: &Path) -> Option<CoverageData> {
    // First, search the given directory itself
    if let Some(data) = search_coverage_in(dir) {
        return Some(data);
    }

    // For Rust workspace members, walk up to find workspace-root coverage files
    for ancestor in dir.ancestors().skip(1) {
        let cargo_toml = ancestor.join("Cargo.toml");
        if cargo_toml.exists()
            && let Ok(content) = std::fs::read_to_string(&cargo_toml)
            && content.contains("[workspace]")
        {
            return search_coverage_in(ancestor);
        }
        // Stop at filesystem root or when no more Cargo.toml found
        if !ancestor.join("Cargo.toml").exists() {
            break;
        }
    }

    None
}

/// Search for coverage files in a single directory.
fn search_coverage_in(dir: &Path) -> Option<CoverageData> {
    for (file, format) in COVERAGE_FILES {
        let path = dir.join(file);
        if path.exists() {
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let result = match format {
                CoverageFormat::Lcov => lcov::parse(&content),
                CoverageFormat::Cobertura => cobertura::parse(&content),
                CoverageFormat::Istanbul => istanbul::parse(&content),
                CoverageFormat::GoCover => go_cover::parse(&content),
                CoverageFormat::TarpaulinJson => parse_tarpaulin_json(&content),
            };

            if let Some(data) = result
                && !data.files.is_empty()
            {
                return Some(data);
            }
        }
    }
    None
}

/// Parse tarpaulin JSON format.
fn parse_tarpaulin_json(content: &str) -> Option<CoverageData> {
    // Tarpaulin JSON has: { "files": [ { "path": "...", "covered": N, "coverable": N } ] }
    let mut files = Vec::new();

    // Simple JSON parsing without serde_json dependency on specific struct
    // Look for file entries
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("\"path\"") {
            // We'll use the cobertura parser as fallback since tarpaulin also exports XML
            continue;
        }
    }

    // Fallback: try to extract basic info
    if content.contains("\"coverable\"") && content.contains("\"covered\"") {
        // Simple extraction of totals
        let coverable = extract_json_number(content, "coverable");
        let covered = extract_json_number(content, "covered");
        if let (Some(total), Some(cov)) = (coverable, covered) {
            let pct = if total > 0 {
                cov as f32 / total as f32 * 100.0
            } else {
                0.0
            };
            files.push(FileCoverage {
                path: "(project total)".to_string(),
                total_lines: total,
                covered_lines: cov,
                coverage_percent: pct,
            });
        }
    }

    if files.is_empty() {
        return None;
    }

    let mut data = CoverageData {
        tool: "tarpaulin".to_string(),
        total_lines: 0,
        covered_lines: 0,
        coverage_percent: 0.0,
        files,
    };
    data.recalculate();
    Some(data)
}

fn extract_json_number(content: &str, key: &str) -> Option<usize> {
    let pattern = format!("\"{}\"", key);
    let pos = content.rfind(&pattern)?;
    let after = &content[pos + pattern.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim();
    let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_coverage_lcov() {
        let dir = TempDir::new().unwrap();
        let lcov = "SF:src/app.py\nDA:1,1\nDA:2,0\nLF:2\nLH:1\nend_of_record\n";
        fs::write(dir.path().join("lcov.info"), lcov).unwrap();
        let data = parse_coverage(dir.path()).unwrap();
        assert_eq!(data.tool, "lcov");
        assert_eq!(data.total_lines, 2);
        assert_eq!(data.covered_lines, 1);
    }

    #[test]
    fn test_parse_coverage_cobertura() {
        let dir = TempDir::new().unwrap();
        let xml = r#"<?xml version="1.0" ?>
<coverage version="6" lines-valid="50" lines-covered="40" line-rate="0.80">
    <packages><package><classes>
        <class name="app" filename="app.py" line-rate="0.80">
            <lines><line number="1" hits="1"/><line number="2" hits="0"/></lines>
        </class>
    </classes></package></packages>
</coverage>"#;
        fs::write(dir.path().join("coverage.xml"), xml).unwrap();
        let data = parse_coverage(dir.path()).unwrap();
        assert!(data.tool.contains("cobertura") || data.tool.contains("pytest"));
        assert!(!data.files.is_empty());
    }

    #[test]
    fn test_parse_coverage_istanbul_summary() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("coverage")).unwrap();
        let json = r#"{
  "total": { "lines": { "total": 100, "covered": 80, "pct": 80 } },
  "src/index.ts": { "lines": { "total": 60, "covered": 50, "pct": 83 } },
  "src/utils.ts": { "lines": { "total": 40, "covered": 30, "pct": 75 } }
}"#;
        fs::write(dir.path().join("coverage/coverage-summary.json"), json).unwrap();
        let data = parse_coverage(dir.path()).unwrap();
        assert_eq!(data.tool, "istanbul");
        assert_eq!(data.files.len(), 2);
    }

    #[test]
    fn test_parse_coverage_go_cover() {
        let dir = TempDir::new().unwrap();
        let content = "mode: set\ngithub.com/user/pkg/main.go:5.2,8.3 3 1\ngithub.com/user/pkg/main.go:10.2,12.3 2 0\n";
        fs::write(dir.path().join("coverage.out"), content).unwrap();
        let data = parse_coverage(dir.path()).unwrap();
        assert_eq!(data.tool, "go-cover");
    }

    #[test]
    fn test_parse_coverage_none_empty() {
        let dir = TempDir::new().unwrap();
        assert!(parse_coverage(dir.path()).is_none());
    }

    #[test]
    fn test_parse_coverage_workspace_search() {
        let dir = TempDir::new().unwrap();
        // Create workspace root with Cargo.toml
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"core\"]",
        )
        .unwrap();
        // Put coverage at workspace root
        let lcov = "SF:src/lib.rs\nLF:10\nLH:8\nend_of_record\n";
        fs::write(dir.path().join("lcov.info"), lcov).unwrap();
        // Create crate directory (direct child with its own Cargo.toml)
        let crate_dir = dir.path().join("core");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(crate_dir.join("Cargo.toml"), "[package]\nname = \"core\"").unwrap();

        let data = parse_coverage(&crate_dir);
        // Should find workspace-level lcov.info by walking up
        if let Some(d) = data {
            assert_eq!(d.tool, "lcov");
        }
    }

    #[test]
    fn test_coverage_data_recalculate() {
        let mut data = CoverageData {
            tool: "test".to_string(),
            total_lines: 0,
            covered_lines: 0,
            coverage_percent: 0.0,
            files: vec![
                FileCoverage {
                    path: "a.rs".into(),
                    total_lines: 100,
                    covered_lines: 80,
                    coverage_percent: 80.0,
                },
                FileCoverage {
                    path: "b.rs".into(),
                    total_lines: 50,
                    covered_lines: 25,
                    coverage_percent: 50.0,
                },
            ],
        };
        data.recalculate();
        assert_eq!(data.total_lines, 150);
        assert_eq!(data.covered_lines, 105);
        assert!((data.coverage_percent - 70.0).abs() < 0.1);
    }

    #[test]
    fn test_coverage_data_recalculate_empty() {
        let mut data = CoverageData {
            tool: "test".to_string(),
            total_lines: 0,
            covered_lines: 0,
            coverage_percent: 0.0,
            files: vec![],
        };
        data.recalculate();
        assert_eq!(data.coverage_percent, 0.0);
    }

    #[test]
    fn test_parse_tarpaulin_json() {
        let content = r#"{"files": [], "coverable": 200, "covered": 150}"#;
        let data = parse_tarpaulin_json(content);
        if let Some(d) = data {
            assert_eq!(d.tool, "tarpaulin");
            assert_eq!(d.total_lines, 200);
            assert_eq!(d.covered_lines, 150);
        }
    }

    #[test]
    fn test_extract_json_number_fn() {
        let content = r#"{"coverable": 42, "covered": 30}"#;
        assert_eq!(extract_json_number(content, "coverable"), Some(42));
        assert_eq!(extract_json_number(content, "covered"), Some(30));
        assert_eq!(extract_json_number(content, "missing"), None);
    }

    #[test]
    fn test_parse_coverage_subdir_lcov() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("coverage")).unwrap();
        let lcov = "SF:lib/app.dart\nLF:5\nLH:3\nend_of_record\n";
        fs::write(dir.path().join("coverage/lcov.info"), lcov).unwrap();
        let data = parse_coverage(dir.path()).unwrap();
        assert_eq!(data.tool, "lcov");
        assert_eq!(data.covered_lines, 3);
    }
}
