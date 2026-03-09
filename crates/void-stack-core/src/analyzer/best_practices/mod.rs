//! Best practices analyzer — delegates to native ecosystem linters
//! (react-doctor, ruff, cargo clippy, golangci-lint, dart/flutter analyze),
//! parses their structured output, and produces unified findings.

pub mod react;
pub mod python;
pub mod rust_bp;
pub mod go_bp;
pub mod flutter;
pub mod report;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A single best practices finding from a native linter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestPracticesFinding {
    pub rule_id: String,
    pub tool: String,
    pub category: BpCategory,
    pub severity: BpSeverity,
    pub file: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
    pub fix_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BpCategory {
    Performance,
    Correctness,
    Style,
    Complexity,
    DeadCode,
    BundleSize,
    Idiom,
    Accessibility,
}

impl std::fmt::Display for BpCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Performance => write!(f, "Performance"),
            Self::Correctness => write!(f, "Correctness"),
            Self::Style => write!(f, "Style"),
            Self::Complexity => write!(f, "Complexity"),
            Self::DeadCode => write!(f, "Dead Code"),
            Self::BundleSize => write!(f, "Bundle Size"),
            Self::Idiom => write!(f, "Idiom"),
            Self::Accessibility => write!(f, "Accessibility"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BpSeverity {
    Important,
    Warning,
    Suggestion,
}

impl std::fmt::Display for BpSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Important => write!(f, "Important"),
            Self::Warning => write!(f, "Warning"),
            Self::Suggestion => write!(f, "Suggestion"),
        }
    }
}

/// Per-language sub-score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolScore {
    pub tool: String,
    pub score: f32,
    pub finding_count: usize,
    /// Native tool score (e.g., react-doctor's 0-100), if available.
    pub native_score: Option<f32>,
}

/// Full result of a best practices analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestPracticesResult {
    pub findings: Vec<BestPracticesFinding>,
    pub overall_score: f32,
    pub tool_scores: Vec<ToolScore>,
    pub tools_used: Vec<String>,
}

impl BestPracticesResult {
    pub fn compute_scores(&mut self) {
        // Global score: 100 - (Important×5 + Warning×2 + Suggestion×0.5)
        let penalty: f32 = self.findings.iter().map(|f| match f.severity {
            BpSeverity::Important => 5.0,
            BpSeverity::Warning => 2.0,
            BpSeverity::Suggestion => 0.5,
        }).sum();
        self.overall_score = (100.0 - penalty).max(0.0);

        // Per-tool scores
        let mut tool_map: std::collections::HashMap<String, Vec<&BestPracticesFinding>> = std::collections::HashMap::new();
        for f in &self.findings {
            tool_map.entry(f.tool.clone()).or_default().push(f);
        }

        for tool_name in &self.tools_used {
            let findings = tool_map.get(tool_name);
            let count = findings.map(|f| f.len()).unwrap_or(0);
            let penalty: f32 = findings.map(|fs| fs.iter().map(|f| match f.severity {
                BpSeverity::Important => 5.0,
                BpSeverity::Warning => 2.0,
                BpSeverity::Suggestion => 0.5,
            }).sum()).unwrap_or(0.0);
            let score = (100.0 - penalty).max(0.0);

            // Check if we already have a native score for this tool
            let existing = self.tool_scores.iter().find(|s| s.tool == *tool_name);
            let native = existing.and_then(|s| s.native_score);

            // Remove old entry if exists, then add updated one
            self.tool_scores.retain(|s| s.tool != *tool_name);
            self.tool_scores.push(ToolScore {
                tool: tool_name.clone(),
                score,
                finding_count: count,
                native_score: native,
            });
        }
    }
}

// ── Subprocess helpers (matching audit/deps.rs pattern) ──────

pub(crate) fn run_command_timeout(cmd: &str, args: &[&str], cwd: &Path, timeout_secs: u64) -> Option<String> {
    use crate::process_util::HideWindow;
    let child = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .hide_window()
        .spawn()
        .ok()?;

    let output = wait_with_timeout(child, Duration::from_secs(timeout_secs))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if stdout.is_empty() && !stderr.is_empty() {
        Some(stderr)
    } else {
        Some(stdout)
    }
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<std::process::Output> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return None,
        }
    }
}

/// Outcome of running a single linter on a directory.
struct LinterOutput {
    tool_name: String,
    findings: Vec<BestPracticesFinding>,
    native_score: Option<f32>,
}

/// A linter definition: relevance check + runner.
struct LinterDef {
    is_relevant: fn(&Path) -> bool,
    run: fn(&Path) -> LinterOutput,
}

fn run_react(path: &Path) -> LinterOutput {
    let (findings, native_score) = react::run_react_doctor(path);
    LinterOutput { tool_name: "react-doctor".into(), findings, native_score }
}

fn run_ruff(path: &Path) -> LinterOutput {
    LinterOutput { tool_name: "ruff".into(), findings: python::run_ruff(path), native_score: None }
}

fn run_clippy(path: &Path) -> LinterOutput {
    LinterOutput { tool_name: "clippy".into(), findings: rust_bp::run_clippy(path), native_score: None }
}

fn run_golangci(path: &Path) -> LinterOutput {
    LinterOutput { tool_name: "golangci-lint".into(), findings: go_bp::run_golangci_lint(path), native_score: None }
}

fn run_dart(path: &Path) -> LinterOutput {
    LinterOutput { tool_name: "dart-analyze".into(), findings: flutter::run_dart_analyze(path), native_score: None }
}

/// All registered linters.
fn linter_defs() -> Vec<LinterDef> {
    vec![
        LinterDef { is_relevant: react::is_relevant, run: run_react },
        LinterDef { is_relevant: python::is_relevant, run: run_ruff },
        LinterDef { is_relevant: rust_bp::is_relevant, run: run_clippy },
        LinterDef { is_relevant: go_bp::is_relevant, run: run_golangci },
        LinterDef { is_relevant: flutter::is_relevant, run: run_dart },
    ]
}

/// Merge a linter output into the result, registering the tool if needed.
fn merge_linter_output(result: &mut BestPracticesResult, output: LinterOutput) {
    let dominated_by_findings = !output.findings.is_empty();
    let has_native = output.native_score.is_some();

    if dominated_by_findings || has_native {
        if !result.tools_used.contains(&output.tool_name) {
            result.tools_used.push(output.tool_name.clone());
        }
        if let Some(ns) = output.native_score {
            if !result.tool_scores.iter().any(|s| s.tool == output.tool_name) {
                result.tool_scores.push(ToolScore {
                    tool: output.tool_name,
                    score: 0.0, // will be recomputed
                    finding_count: 0,
                    native_score: Some(ns),
                });
            }
        }
    }
    result.findings.extend(output.findings);
}

/// Returns true if the subdirectory name should be skipped during monorepo scanning.
fn is_skippable_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "target" || name == "__pycache__"
}

/// Run all applicable best practices tools on a project directory.
pub fn analyze_best_practices(project_path: &Path) -> BestPracticesResult {
    let mut result = BestPracticesResult {
        findings: Vec::new(),
        overall_score: 100.0,
        tool_scores: Vec::new(),
        tools_used: Vec::new(),
    };

    let linters = linter_defs();

    // Run each linter on the root project directory.
    for linter in &linters {
        if (linter.is_relevant)(project_path) {
            let output = (linter.run)(project_path);
            merge_linter_output(&mut result, output);
        }
    }

    // Also scan 1 level of subdirectories for monorepos.
    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let sub = entry.path();
            if !sub.is_dir() { continue; }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if is_skippable_dir(&name_str) { continue; }

            for linter in &linters {
                // Only run on subdirectory if the root wasn't already relevant.
                if (linter.is_relevant)(&sub) && !(linter.is_relevant)(project_path) {
                    let output = (linter.run)(&sub);
                    merge_linter_output(&mut result, output);
                }
            }
        }
    }

    result.compute_scores();
    result
}

