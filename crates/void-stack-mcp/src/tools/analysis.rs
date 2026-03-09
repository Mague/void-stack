use std::collections::HashMap;

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

use super::to_json_pretty;

/// Logic for analyze_project tool.
pub fn analyze_project(
    project: &Project,
    service_name: Option<&str>,
    best_practices: bool,
) -> Result<CallToolResult, McpError> {
    let mut results = Vec::new();
    let services: Vec<_> = match service_name {
        Some(svc_name) => project
            .services
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case(svc_name))
            .collect(),
        None => project.services.iter().collect(),
    };

    for svc in &services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let clean = strip_win_prefix(dir);
        let path = std::path::Path::new(&clean);
        if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
            let doc = void_stack_core::analyzer::generate_docs(&result, &svc.name);
            results.push(doc);
        }
    }

    if results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No analyzable code found (supported: Python, JavaScript/TypeScript)".to_string(),
        )]));
    }

    let mut full = results.join("\n\n---\n\n");

    // Best practices analysis if requested
    if best_practices {
        let dir = strip_win_prefix(&project.path);
        let bp_result = void_stack_core::analyzer::best_practices::analyze_best_practices(
            std::path::Path::new(&dir),
        );
        let bp_md =
            void_stack_core::analyzer::best_practices::report::generate_best_practices_markdown(
                &bp_result,
            );
        full.push_str("\n\n");
        full.push_str(&bp_md);
    }

    // Save to project dir
    let dir = strip_win_prefix(&project.path);
    let path = format!("{}/void-stack-analysis.md", dir);
    let _ = std::fs::write(&path, &full);

    Ok(CallToolResult::success(vec![Content::text(full)]))
}

/// Logic for audit_project tool.
pub fn audit_project(project: &Project) -> Result<CallToolResult, McpError> {
    let clean_path = strip_win_prefix(&project.path);
    let result =
        void_stack_core::audit::audit_project(&project.name, std::path::Path::new(&clean_path));

    let json = to_json_pretty(&result)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Logic for check_dependencies tool.
pub async fn check_dependencies(project: &Project) -> Result<CallToolResult, McpError> {
    // Collect all unique directories
    let mut dirs: Vec<std::path::PathBuf> = vec![];
    let root = strip_win_prefix(&project.path);
    dirs.push(std::path::PathBuf::from(&root));

    for svc in &project.services {
        if let Some(dir) = &svc.working_dir {
            let stripped = strip_win_prefix(dir);
            let p = std::path::PathBuf::from(&stripped);
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut all_results = Vec::new();

    for dir in &dirs {
        let results = void_stack_core::detector::check_project(dir).await;
        for result in results {
            if seen.insert(format!("{:?}", result.dep_type)) {
                all_results.push(result);
            }
        }
    }

    let json = to_json_pretty(&all_results)?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Logic for analyze_cross_project tool.
pub fn analyze_cross_project(
    config: &void_stack_core::global_config::GlobalConfig,
) -> Result<CallToolResult, McpError> {
    if config.projects.len() < 2 {
        return Ok(CallToolResult::success(vec![Content::text(
            "Need at least 2 registered projects to detect cross-project coupling.".to_string(),
        )]));
    }

    // Analyze all projects
    let mut analysis_results: HashMap<
        String,
        Vec<(String, void_stack_core::analyzer::AnalysisResult)>,
    > = HashMap::new();

    for project in &config.projects {
        let mut svc_results = Vec::new();
        for svc in &project.services {
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let clean = strip_win_prefix(dir);
            let path = std::path::Path::new(&clean);
            if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
                svc_results.push((svc.name.clone(), result));
            }
        }
        if !svc_results.is_empty() {
            analysis_results.insert(project.name.clone(), svc_results);
        }
    }

    if analysis_results.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No analyzable code found in any project.".to_string(),
        )]));
    }

    let result =
        void_stack_core::analyzer::analyze_cross_project(&config.projects, &analysis_results);

    let mut output = String::new();
    output.push_str("## Cross-Project Coupling Analysis\n\n");

    if result.links.is_empty() {
        output.push_str("No cross-project dependencies detected.\n");
    } else {
        output.push_str(&format!(
            "Found {} cross-project link(s):\n\n",
            result.links.len()
        ));
        output.push_str("| From Project | Service | To Project | Via Dependency |\n");
        output.push_str("|-------------|---------|------------|----------------|\n");
        for link in &result.links {
            output.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                link.from_project, link.from_service, link.to_project, link.via_dependency,
            ));
        }
    }

    if !result.unmatched_external.is_empty() {
        let mut ext: Vec<_> = result.unmatched_external.iter().collect();
        ext.sort();
        let shown = ext
            .iter()
            .take(30)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        output.push_str(&format!(
            "\n**External dependencies** (not matching any project): {} total\n{}{}\n",
            ext.len(),
            shown,
            if ext.len() > 30 { " ..." } else { "" },
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
