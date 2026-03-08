//! Code analysis: dependency graphs, architecture patterns, anti-patterns, coverage, documentation.

pub mod graph;
pub mod imports;
pub mod patterns;
pub mod coverage;
pub mod docs;

use std::path::Path;

use graph::DependencyGraph;
use patterns::ArchAnalysis;
use coverage::CoverageData;

/// Full analysis result for a project or service directory.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub graph: DependencyGraph,
    pub architecture: ArchAnalysis,
    pub coverage: Option<CoverageData>,
}

/// Analyze a project directory: build dependency graph, detect patterns and anti-patterns.
/// Optionally parses test coverage if coverage data files are found.
pub fn analyze_project(project_path: &Path) -> Option<AnalysisResult> {
    let graph = imports::build_graph(project_path)?;
    let architecture = patterns::detect_architecture(&graph);
    let coverage = coverage::parse_coverage(project_path);

    Some(AnalysisResult {
        graph,
        architecture,
        coverage,
    })
}

/// Generate markdown documentation from analysis results.
pub fn generate_docs(result: &AnalysisResult, project_name: &str) -> String {
    docs::generate_docs(result, project_name)
}
