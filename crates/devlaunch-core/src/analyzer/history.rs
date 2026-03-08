//! Technical debt tracking — snapshot comparison across versions.
//!
//! Stores analysis snapshots in `.devlaunch/history/` within the project directory.
//! Each snapshot contains metrics that can be compared over time.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A point-in-time snapshot of analysis metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSnapshot {
    pub timestamp: DateTime<Utc>,
    /// Optional label (git tag, version, etc.)
    pub label: Option<String>,
    pub services: Vec<ServiceSnapshot>,
}

/// Metrics for a single service at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSnapshot {
    pub name: String,
    pub pattern: String,
    pub confidence: f32,
    pub total_modules: usize,
    pub total_loc: usize,
    pub external_deps: usize,
    pub anti_pattern_count: usize,
    pub anti_patterns_high: usize,
    pub anti_patterns_medium: usize,
    pub anti_patterns_low: usize,
    /// Average cyclomatic complexity.
    pub avg_complexity: f32,
    /// Max cyclomatic complexity.
    pub max_complexity: usize,
    /// Number of functions with complexity >= 10.
    pub complex_functions: usize,
    /// Coverage percent (if available).
    pub coverage_percent: Option<f32>,
    /// God class count.
    pub god_classes: usize,
    /// Circular dependency count.
    pub circular_deps: usize,
}

/// Comparison between two snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebtComparison {
    pub previous: DateTime<Utc>,
    pub current: DateTime<Utc>,
    pub services: Vec<ServiceComparison>,
    pub overall_trend: Trend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceComparison {
    pub name: String,
    pub loc_delta: i64,
    pub antipattern_delta: i32,
    pub complexity_delta: f32,
    pub coverage_delta: Option<f32>,
    pub god_class_delta: i32,
    pub circular_dep_delta: i32,
    pub trend: Trend,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Trend {
    Improving,
    Stable,
    Degrading,
}

impl std::fmt::Display for Trend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Trend::Improving => write!(f, "Mejorando"),
            Trend::Stable => write!(f, "Estable"),
            Trend::Degrading => write!(f, "Degradando"),
        }
    }
}

/// Directory where snapshots are stored.
fn history_dir(project_path: &Path) -> std::path::PathBuf {
    project_path.join(".devlaunch").join("history")
}

/// Save a snapshot to disk.
pub fn save_snapshot(project_path: &Path, snapshot: &AnalysisSnapshot) -> std::io::Result<()> {
    let dir = history_dir(project_path);
    std::fs::create_dir_all(&dir)?;

    let filename = format!("{}.json", snapshot.timestamp.format("%Y%m%d_%H%M%S"));
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}

/// Load all snapshots sorted by time (oldest first).
pub fn load_snapshots(project_path: &Path) -> Vec<AnalysisSnapshot> {
    let dir = history_dir(project_path);
    let mut snapshots = Vec::new();

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return snapshots,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(snap) = serde_json::from_str::<AnalysisSnapshot>(&content) {
                    snapshots.push(snap);
                }
            }
        }
    }

    snapshots.sort_by_key(|s| s.timestamp);
    snapshots
}

/// Load only the most recent snapshot.
pub fn load_latest(project_path: &Path) -> Option<AnalysisSnapshot> {
    load_snapshots(project_path).into_iter().last()
}

/// Create a snapshot from analysis results.
pub fn create_snapshot(
    results: &[(String, super::AnalysisResult)],
    label: Option<String>,
) -> AnalysisSnapshot {
    use super::patterns::antipatterns::{AntiPatternKind, Severity};

    let services = results
        .iter()
        .map(|(name, result)| {
            let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();
            let anti_patterns = &result.architecture.anti_patterns;

            let god_classes = anti_patterns
                .iter()
                .filter(|a| a.kind == AntiPatternKind::GodClass)
                .count();
            let circular_deps = anti_patterns
                .iter()
                .filter(|a| a.kind == AntiPatternKind::CircularDependency)
                .count();

            // Complexity from modules
            let (avg_cx, max_cx, complex_fns) = if let Some(cx) = &result.complexity {
                let total: usize = cx.iter().map(|(_, fc)| {
                    fc.functions.iter().map(|f| f.complexity).sum::<usize>()
                }).sum();
                let fn_count: usize = cx.iter().map(|(_, fc)| fc.functions.len()).sum();
                let avg = if fn_count > 0 { total as f32 / fn_count as f32 } else { 0.0 };
                let max = cx.iter()
                    .flat_map(|(_, fc)| fc.functions.iter())
                    .map(|f| f.complexity)
                    .max()
                    .unwrap_or(0);
                let complex = cx.iter()
                    .flat_map(|(_, fc)| fc.functions.iter())
                    .filter(|f| f.complexity >= 10)
                    .count();
                (avg, max, complex)
            } else {
                (0.0, 0, 0)
            };

            ServiceSnapshot {
                name: name.clone(),
                pattern: result.architecture.detected_pattern.to_string(),
                confidence: result.architecture.confidence,
                total_modules: result.graph.modules.len(),
                total_loc,
                external_deps: result.graph.external_deps.len(),
                anti_pattern_count: anti_patterns.len(),
                anti_patterns_high: anti_patterns.iter().filter(|a| a.severity == Severity::High).count(),
                anti_patterns_medium: anti_patterns.iter().filter(|a| a.severity == Severity::Medium).count(),
                anti_patterns_low: anti_patterns.iter().filter(|a| a.severity == Severity::Low).count(),
                avg_complexity: avg_cx,
                max_complexity: max_cx,
                complex_functions: complex_fns,
                coverage_percent: result.coverage.as_ref().map(|c| c.coverage_percent),
                god_classes,
                circular_deps,
            }
        })
        .collect();

    AnalysisSnapshot {
        timestamp: Utc::now(),
        label,
        services,
    }
}

/// Compare two snapshots.
pub fn compare(previous: &AnalysisSnapshot, current: &AnalysisSnapshot) -> DebtComparison {
    let mut services = Vec::new();
    let mut improving = 0i32;
    let mut degrading = 0i32;

    for curr_svc in &current.services {
        let prev_svc = previous.services.iter().find(|s| s.name == curr_svc.name);

        let comparison = if let Some(prev) = prev_svc {
            let loc_delta = curr_svc.total_loc as i64 - prev.total_loc as i64;
            let antipattern_delta = curr_svc.anti_pattern_count as i32 - prev.anti_pattern_count as i32;
            let complexity_delta = curr_svc.avg_complexity - prev.avg_complexity;
            let god_class_delta = curr_svc.god_classes as i32 - prev.god_classes as i32;
            let circular_dep_delta = curr_svc.circular_deps as i32 - prev.circular_deps as i32;
            let coverage_delta = match (curr_svc.coverage_percent, prev.coverage_percent) {
                (Some(c), Some(p)) => Some(c - p),
                _ => None,
            };

            // Determine trend based on weighted score
            let mut score: f32 = 0.0;
            score += antipattern_delta as f32 * 2.0;
            score += complexity_delta;
            score += god_class_delta as f32 * 3.0;
            score += circular_dep_delta as f32 * 3.0;
            if let Some(cd) = coverage_delta {
                score -= cd * 0.5; // More coverage = better (subtract)
            }

            let trend = if score < -1.0 {
                improving += 1;
                Trend::Improving
            } else if score > 1.0 {
                degrading += 1;
                Trend::Degrading
            } else {
                Trend::Stable
            };

            ServiceComparison {
                name: curr_svc.name.clone(),
                loc_delta,
                antipattern_delta,
                complexity_delta,
                coverage_delta,
                god_class_delta,
                circular_dep_delta,
                trend,
            }
        } else {
            // New service, no comparison
            ServiceComparison {
                name: curr_svc.name.clone(),
                loc_delta: curr_svc.total_loc as i64,
                antipattern_delta: curr_svc.anti_pattern_count as i32,
                complexity_delta: curr_svc.avg_complexity,
                coverage_delta: curr_svc.coverage_percent,
                god_class_delta: curr_svc.god_classes as i32,
                circular_dep_delta: curr_svc.circular_deps as i32,
                trend: Trend::Stable,
            }
        };

        services.push(comparison);
    }

    let overall_trend = if improving > degrading {
        Trend::Improving
    } else if degrading > improving {
        Trend::Degrading
    } else {
        Trend::Stable
    };

    DebtComparison {
        previous: previous.timestamp,
        current: current.timestamp,
        services,
        overall_trend,
    }
}

/// Generate markdown report for a comparison.
pub fn comparison_markdown(comp: &DebtComparison) -> String {
    let mut md = String::new();

    md.push_str("## Comparacion de Deuda Tecnica\n\n");
    md.push_str(&format!(
        "**Anterior**: {} | **Actual**: {} | **Tendencia**: {}\n\n",
        comp.previous.format("%Y-%m-%d %H:%M"),
        comp.current.format("%Y-%m-%d %H:%M"),
        comp.overall_trend,
    ));

    md.push_str("| Servicio | LOC | Anti-patrones | Complejidad | Coverage | Tendencia |\n");
    md.push_str("|----------|-----|---------------|-------------|----------|----------|\n");

    for svc in &comp.services {
        let loc_str = format_delta_i64(svc.loc_delta);
        let ap_str = format_delta_i32(svc.antipattern_delta);
        let cx_str = format_delta_f32(svc.complexity_delta);
        let cov_str = svc.coverage_delta
            .map(|d| format_delta_f32(d))
            .unwrap_or_else(|| "-".to_string());
        let trend_icon = match svc.trend {
            Trend::Improving => "mejorando",
            Trend::Stable => "estable",
            Trend::Degrading => "degradando",
        };

        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            svc.name, loc_str, ap_str, cx_str, cov_str, trend_icon
        ));
    }

    md.push_str("\n");
    md
}

fn format_delta_i64(v: i64) -> String {
    if v > 0 { format!("+{}", v) }
    else if v < 0 { format!("{}", v) }
    else { "=".to_string() }
}

fn format_delta_i32(v: i32) -> String {
    if v > 0 { format!("+{}", v) }
    else if v < 0 { format!("{}", v) }
    else { "=".to_string() }
}

fn format_delta_f32(v: f32) -> String {
    if v > 0.1 { format!("+{:.1}", v) }
    else if v < -0.1 { format!("{:.1}", v) }
    else { "=".to_string() }
}
