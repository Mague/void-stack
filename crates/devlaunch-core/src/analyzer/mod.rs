//! Code analysis: dependency graphs, architecture patterns, anti-patterns, coverage,
//! complexity, technical debt tracking, cross-project coupling, documentation.

pub mod graph;
pub mod imports;
pub mod patterns;
pub mod coverage;
pub mod complexity;
pub mod history;
pub mod cross_project;
pub mod docs;
pub mod best_practices;

use std::collections::HashMap;
use std::path::Path;

use graph::DependencyGraph;
use patterns::ArchAnalysis;
use coverage::CoverageData;
use complexity::FileComplexity;

/// Full analysis result for a project or service directory.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub graph: DependencyGraph,
    pub architecture: ArchAnalysis,
    pub coverage: Option<CoverageData>,
    /// Per-file complexity analysis: (relative_path -> FileComplexity).
    pub complexity: Option<Vec<(String, FileComplexity)>>,
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

    let complexity = if complexity_data.is_empty() {
        None
    } else {
        Some(complexity_data)
    };

    Some(AnalysisResult {
        graph,
        architecture,
        coverage,
        complexity,
    })
}

/// Generate markdown documentation from analysis results.
pub fn generate_docs(result: &AnalysisResult, project_name: &str) -> String {
    docs::generate_docs(result, project_name)
}

/// Perform cross-project coupling analysis across all registered projects.
pub fn analyze_cross_project(
    projects: &[crate::model::Project],
    analysis_results: &HashMap<String, Vec<(String, AnalysisResult)>>,
) -> cross_project::CrossProjectResult {
    let identifiers = cross_project::build_identifiers(projects);

    let mut project_deps: HashMap<String, Vec<(String, std::collections::HashSet<String>)>> = HashMap::new();
    for (proj_name, service_results) in analysis_results {
        let mut svc_deps = Vec::new();
        for (svc_name, result) in service_results {
            svc_deps.push((svc_name.clone(), result.graph.external_deps.clone()));
        }
        project_deps.insert(proj_name.clone(), svc_deps);
    }

    cross_project::detect_cross_project(&project_deps, &identifiers)
}
