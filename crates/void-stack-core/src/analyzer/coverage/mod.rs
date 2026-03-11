//! Test coverage parsing and visualization.
//!
//! Supports: LCOV (Flutter, genhtml), Cobertura XML (pytest-cov, tarpaulin),
//! Istanbul JSON (c8/nyc), Go cover profiles.

pub mod lcov;
pub mod cobertura;
pub mod istanbul;
pub mod go_cover;

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
        if cargo_toml.exists() {
            if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
                if content.contains("[workspace]") {
                    return search_coverage_in(ancestor);
                }
            }
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

            if let Some(data) = result {
                if !data.files.is_empty() {
                    return Some(data);
                }
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
            let pct = if total > 0 { cov as f32 / total as f32 * 100.0 } else { 0.0 };
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
