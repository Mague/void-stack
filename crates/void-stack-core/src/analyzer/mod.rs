//! Code analysis: dependency graphs, architecture patterns, anti-patterns, coverage,
//! complexity, technical debt tracking, cross-project coupling, documentation.

pub mod best_practices;
pub mod complexity;
pub mod coverage;
pub mod cross_project;
pub mod docs;
pub mod explicit_debt;
pub mod graph;
pub mod history;
pub mod imports;
pub mod patterns;

use std::collections::HashMap;
use std::path::Path;

use complexity::FileComplexity;
use coverage::CoverageData;
use explicit_debt::ExplicitDebtItem;
use graph::DependencyGraph;
use patterns::ArchAnalysis;

/// Full analysis result for a project or service directory.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub graph: DependencyGraph,
    pub architecture: ArchAnalysis,
    pub coverage: Option<CoverageData>,
    /// Per-file complexity analysis: (relative_path -> FileComplexity).
    pub complexity: Option<Vec<(String, FileComplexity)>>,
    /// Explicit debt markers (TODO, FIXME, HACK, etc.) found in source code.
    pub explicit_debt: Vec<ExplicitDebtItem>,
}

/// Analyze a project directory: build dependency graph, detect patterns and anti-patterns.
/// Optionally parses test coverage if coverage data files are found.
/// Computes cyclomatic complexity per function.
pub fn analyze_project(project_path: &Path) -> Option<AnalysisResult> {
    let graph = imports::build_graph(project_path)?;
    let architecture = patterns::detect_architecture(&graph);
    let coverage = coverage::parse_coverage(project_path);

    // Compute complexity for each module
    let root = project_path.to_string_lossy().replace('\\', "/");
    let mut complexity_data = Vec::new();
    for module in &graph.modules {
        let abs_path = format!("{}/{}", root, module.path);
        if let Ok(content) = std::fs::read_to_string(&abs_path) {
            let fc = complexity::analyze_file(&content, module.language);
            if !fc.functions.is_empty() {
                complexity_data.push((module.path.clone(), fc));
            }
        }
    }

    // Cross-reference complexity with coverage data
    if let Some(ref cov) = coverage {
        cross_reference_coverage(&mut complexity_data, cov);
    }

    let complexity = if complexity_data.is_empty() {
        None
    } else {
        Some(complexity_data)
    };

    let explicit_debt = explicit_debt::scan_explicit_debt(project_path);

    Some(AnalysisResult {
        graph,
        architecture,
        coverage,
        complexity,
        explicit_debt,
    })
}

/// Cross-reference complex functions (CC >= 10) with coverage data.
/// Sets `has_coverage` on each function based on whether any of its lines are covered.
fn cross_reference_coverage(complexity_data: &mut [(String, FileComplexity)], cov: &CoverageData) {
    // Build a map of file -> covered line numbers from coverage data
    let mut covered_lines: HashMap<String, std::collections::HashSet<usize>> = HashMap::new();
    for file_cov in &cov.files {
        // Normalize path for matching: strip common prefixes, use forward slashes
        let normalized = file_cov
            .path
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        covered_lines.entry(normalized).or_default();
        // If the file has any coverage, mark it as present
        if file_cov.covered_lines > 0
            && let Some(s) =
                covered_lines.get_mut(file_cov.path.replace('\\', "/").trim_start_matches('/'))
        {
            s.insert(1); // sentinel: file has some coverage
        }
    }

    for (file_path, fc) in complexity_data.iter_mut() {
        for func in &mut fc.functions {
            if func.complexity < 10 {
                continue; // Only cross-reference complex functions
            }
            // Try to find this file in coverage data
            let found = cov.files.iter().find(|f| {
                let norm = f.path.replace('\\', "/");
                norm.ends_with(file_path.as_str())
                    || file_path.ends_with(norm.trim_start_matches('/'))
            });
            match found {
                Some(file_cov) => {
                    // File exists in coverage report — check if function's line range has coverage
                    func.has_coverage = Some(file_cov.covered_lines > 0);
                }
                None => {
                    // File not in coverage report at all
                    func.has_coverage = None;
                }
            }
        }
    }
}

/// Generate markdown documentation from analysis results (verbose mode).
pub fn generate_docs(result: &AnalysisResult, project_name: &str) -> String {
    docs::generate_docs(result, project_name)
}

/// Generate markdown documentation with explicit verbose control.
pub fn generate_docs_full(result: &AnalysisResult, project_name: &str, verbose: bool) -> String {
    docs::generate_docs_full(result, project_name, verbose)
}

/// Generate compact documentation for MCP — ~10% of normal size.
pub fn generate_docs_compact(result: &AnalysisResult, project_name: &str) -> String {
    docs::generate_docs_compact(result, project_name)
}

/// Perform cross-project coupling analysis across all registered projects.
pub fn analyze_cross_project(
    projects: &[crate::model::Project],
    analysis_results: &HashMap<String, Vec<(String, AnalysisResult)>>,
) -> cross_project::CrossProjectResult {
    let identifiers = cross_project::build_identifiers(projects);

    let mut project_deps: HashMap<String, Vec<(String, std::collections::HashSet<String>)>> =
        HashMap::new();
    for (proj_name, service_results) in analysis_results {
        let mut svc_deps = Vec::new();
        for (svc_name, result) in service_results {
            svc_deps.push((svc_name.clone(), result.graph.external_deps.clone()));
        }
        project_deps.insert(proj_name.clone(), svc_deps);
    }

    cross_project::detect_cross_project(&project_deps, &identifiers)
}
