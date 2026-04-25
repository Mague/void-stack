//! `setup_project` — one-shot onboarding for new projects.

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use crate::server::VoidStackMcp;
use crate::types::SetupProjectRequest;

use void_stack_core::runner::local::strip_win_prefix;

pub async fn setup_project(
    _mcp: &VoidStackMcp,
    req: SetupProjectRequest,
) -> Result<CallToolResult, McpError> {
    let start = std::time::Instant::now();
    let mut report = String::new();
    let mut warnings: Vec<String> = Vec::new();

    let path = req.path.trim().to_string();
    let name = req.name.unwrap_or_else(|| {
        std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".into())
    });
    let wsl = req.wsl.unwrap_or(false);

    report.push_str(&format!("# Project Setup — {}\n\n", name));

    // ── Step 1: Register project ────────────────────────────
    report.push_str("## 1. Registration\n\n");
    match super::projects::add_project(&name, &path, wsl, req.distro.as_deref()) {
        Ok(_) => report.push_str(&format!("Registered '{}' at `{}`\n\n", name, path)),
        Err(e) => {
            warnings.push(format!("Registration: {}", e));
            report.push_str(&format!("Warning: {}\n\n", e));
        }
    }

    let project_path = std::path::Path::new(&path);

    // ── Step 2: Generate .claudeignore ──────────────────────
    report.push_str("## 2. .claudeignore\n\n");
    let ci_result = void_stack_core::claudeignore::generate_claudeignore(project_path);
    let ci_count = ci_result
        .content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .count();
    match void_stack_core::claudeignore::save_claudeignore(project_path, &ci_result.content) {
        Ok(_) => report.push_str(&format!(
            "Generated .claudeignore ({} patterns)\n\n",
            ci_count
        )),
        Err(e) => {
            warnings.push(format!(".claudeignore: {}", e));
            report.push_str(&format!("Warning: {}\n\n", e));
        }
    }

    // ── Step 3: Generate .voidignore ────────────────────────
    report.push_str("## 3. .voidignore\n\n");
    let vi = void_stack_core::vector_index::generate_voidignore(project_path);
    match void_stack_core::vector_index::save_voidignore(project_path, &vi.content) {
        Ok(_) => report.push_str(&format!(
            "Generated .voidignore ({} patterns)\n\n",
            vi.patterns_count
        )),
        Err(e) => {
            warnings.push(format!(".voidignore: {}", e));
            report.push_str(&format!("Warning: {}\n\n", e));
        }
    }

    // ── Step 4: Index codebase ──────────────────────────────
    report.push_str("## 4. Semantic Index\n\n");
    let config = VoidStackMcp::load_config()
        .map_err(|e| McpError::internal_error(format!("Config: {}", e), None))?;
    match VoidStackMcp::find_project_or_err(&config, &name) {
        Ok(proj) => {
            match void_stack_core::vector_index::index_project(&proj, false, None, |_, _| {}) {
                Ok(stats) => {
                    report.push_str(&format!(
                        "Indexed **{} files** → **{} chunks** ({:.1} MB)\n\n",
                        stats.files_indexed, stats.chunks_total, stats.size_mb
                    ));
                }
                Err(e) => {
                    warnings.push(format!("Index: {}", e));
                    report.push_str(&format!("Warning: {}\n\n", e));
                }
            }
        }
        Err(e) => {
            warnings.push(format!("Lookup: {}", e));
            report.push_str(&format!("Warning: {}\n\n", e));
        }
    }

    // ── Step 5: Quick analysis ──────────────────────────────
    report.push_str("## 5. Quick Analysis\n\n");
    let clean = strip_win_prefix(&path);
    let audit = void_stack_core::audit::audit_project(&name, std::path::Path::new(&clean));
    report.push_str(&format!(
        "- **Risk:** {:.0}/100\n- Findings: Critical:{} High:{} Medium:{} Low:{} Info:{}\n",
        audit.summary.risk_score,
        audit.summary.critical,
        audit.summary.high,
        audit.summary.medium,
        audit.summary.low,
        audit.summary.info,
    ));
    if let Some(analysis) = void_stack_core::analyzer::analyze_project(std::path::Path::new(&clean))
    {
        report.push_str(&format!(
            "- **Architecture:** {} ({} modules, {} LOC)\n",
            analysis.architecture.detected_pattern,
            analysis.graph.modules.len(),
            analysis.graph.modules.iter().map(|m| m.loc).sum::<usize>(),
        ));
    }
    report.push('\n');

    // ── Step 6: Diagrams (optional) ─────────────────────────
    if req.include_diagrams.unwrap_or(false) {
        report.push_str("## 6. Diagrams\n\n");
        if let Ok(proj) = VoidStackMcp::find_project_or_err(&config, &name) {
            match super::diagrams::generate_diagram(&proj, Some("mermaid")) {
                Ok(_) => report.push_str("Diagram generated (mermaid).\n\n"),
                Err(e) => {
                    warnings.push(format!("Diagram: {}", e));
                    report.push_str(&format!("Warning: {}\n\n", e));
                }
            }
        }
    }

    // ── Summary ─────────────────────────────────────────────
    let elapsed = start.elapsed();
    report.push_str(&format!(
        "\n---\n*Setup completed in {:.1}s",
        elapsed.as_secs_f64()
    ));
    if !warnings.is_empty() {
        report.push_str(&format!(" with {} warning(s)", warnings.len()));
    }
    report.push_str("*\n\n");

    report.push_str("## What you can ask me now\n\n");
    report.push_str(&format!(
        "- `semantic_search project=\"{}\" query=\"authentication flow\"`\n",
        name
    ));
    report.push_str(&format!("- `get_impact_radius project=\"{}\"`\n", name));
    report.push_str(&format!(
        "- `full_analysis project=\"{}\" depth=\"standard\"`\n",
        name
    ));
    report.push_str(&format!("- `generate_diagram project=\"{}\"`\n", name));
    report.push_str(&format!("- `suggest_refactoring project=\"{}\"`\n", name));

    Ok(CallToolResult::success(vec![Content::text(report)]))
}
