use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::runner::local::strip_win_prefix;

// ── Analyze ─────────────────────────────────────────────────

pub fn cmd_analyze(
    project_name: &str,
    output: Option<&str>,
    service_filter: Option<&str>,
    label: Option<&str>,
    do_compare: bool,
    do_cross_project: bool,
    do_best_practices: bool,
    bp_only: bool,
) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let dirs = collect_service_dirs(&project, service_filter)?;

    let mut full_doc = String::new();
    let mut named_results: Vec<(String, void_stack_core::analyzer::AnalysisResult)> = Vec::new();
    let project_path_str = strip_win_prefix(&project.path);

    if !bp_only {
        analyze_services(&dirs, &mut full_doc, &mut named_results);

        let project_path = Path::new(&project_path_str);
        if !named_results.is_empty() {
            handle_snapshot_and_compare(
                project_path,
                &named_results,
                label,
                do_compare,
                &mut full_doc,
            );
        }

        if do_cross_project && !named_results.is_empty() {
            run_cross_project_analysis(
                &config,
                &project,
                &named_results,
                &mut full_doc,
            );
        }
    }

    if do_best_practices {
        run_best_practices(&project_path_str, &mut full_doc);
    }

    save_output(&full_doc, output, &project.path)?;

    Ok(())
}

/// Collect the directories to analyze from the project's services.
fn collect_service_dirs(
    project: &void_stack_core::model::Project,
    service_filter: Option<&str>,
) -> Result<Vec<(String, std::path::PathBuf)>> {
    let mut dirs: Vec<(String, std::path::PathBuf)> = Vec::new();

    match service_filter {
        Some(svc_name) => {
            let svc = project.services.iter()
                .find(|s| s.name.eq_ignore_ascii_case(svc_name))
                .ok_or_else(|| anyhow::anyhow!("Service '{}' not found in project.", svc_name))?;
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let clean = strip_win_prefix(dir);
            dirs.push((svc.name.clone(), Path::new(&clean).to_path_buf()));
        }
        None => {
            for svc in &project.services {
                let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
                let clean = strip_win_prefix(dir);
                dirs.push((svc.name.clone(), Path::new(&clean).to_path_buf()));
            }
            if dirs.is_empty() {
                let clean = strip_win_prefix(&project.path);
                dirs.push((project.name.clone(), Path::new(&clean).to_path_buf()));
            }
        }
    }

    Ok(dirs)
}

/// Analyze each service directory and accumulate results + markdown.
fn analyze_services(
    dirs: &[(String, std::path::PathBuf)],
    full_doc: &mut String,
    named_results: &mut Vec<(String, void_stack_core::analyzer::AnalysisResult)>,
) {
    for (svc_name, dir) in dirs {
        println!("Analyzing {}...", svc_name);

        match void_stack_core::analyzer::analyze_project(dir) {
            Some(result) => {
                let doc = void_stack_core::analyzer::generate_docs(&result, svc_name);
                full_doc.push_str(&doc);
                full_doc.push_str("\n\n---\n\n");

                print_analysis_summary(svc_name, &result);
                named_results.push((svc_name.clone(), result));
            }
            None => {
                println!("  Could not detect language for {}", dir.display());
            }
        }
    }
}

/// Print the console summary for a single service analysis result.
fn print_analysis_summary(svc_name: &str, result: &void_stack_core::analyzer::AnalysisResult) {
    let _ = svc_name; // name already printed by caller
    println!("  Pattern: {} ({:.0}% confidence)",
        result.architecture.detected_pattern,
        result.architecture.confidence * 100.0);
    println!("  Modules: {}", result.graph.modules.len());
    let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();
    println!("  LOC: {}", total_loc);
    println!("  External deps: {}", result.graph.external_deps.len());

    print_complexity_summary(&result.complexity, &result.coverage);
    print_anti_patterns(&result.architecture.anti_patterns);
    print_coverage(&result.coverage);
    print_explicit_debt(&result.explicit_debt);

    println!();
}

/// Print complexity summary if available, with coverage cross-reference.
fn print_complexity_summary(
    complexity: &Option<Vec<(String, void_stack_core::analyzer::complexity::FileComplexity)>>,
    coverage: &Option<void_stack_core::analyzer::coverage::CoverageData>,
) {
    if let Some(cx) = complexity {
        let all_funcs: Vec<_> = cx.iter()
            .flat_map(|(path, fc)| fc.functions.iter().map(move |f| (path.as_str(), f)))
            .collect();
        if !all_funcs.is_empty() {
            let max = all_funcs.iter().max_by_key(|(_, f)| f.complexity).unwrap();
            let complex_count = all_funcs.iter().filter(|(_, f)| f.complexity >= 10).count();
            println!("  Complexity: max {} ({}), {} complex functions",
                max.1.complexity, max.1.name, complex_count);

            // Show critical functions without coverage
            let uncovered: Vec<_> = all_funcs.iter()
                .filter(|(_, f)| f.complexity >= 10 && f.has_coverage == Some(false))
                .collect();
            if !uncovered.is_empty() {
                println!("  Critical functions without coverage:");
                for (path, func) in &uncovered {
                    println!("    [!] {}:{} — {} (CC={})", path, func.line, func.name, func.complexity);
                }
            }

            // If no coverage data at all, show hint
            if coverage.is_none() && complex_count > 0 {
                println!("  Hint: no coverage report found. Generate one to cross-reference critical functions.");
            }
        }
    }
}

/// Print anti-pattern findings.
fn print_anti_patterns(anti_patterns: &[void_stack_core::analyzer::patterns::antipatterns::AntiPattern]) {
    if !anti_patterns.is_empty() {
        println!("  Anti-patterns: {}", anti_patterns.len());
        for ap in anti_patterns {
            println!("    [{:?}] {}: {}", ap.severity, ap.kind, ap.description);
        }
    } else {
        println!("  No anti-patterns detected.");
    }
}

/// Print coverage info if available.
fn print_coverage(coverage: &Option<void_stack_core::analyzer::coverage::CoverageData>) {
    if let Some(cov) = coverage {
        println!("  Coverage: {:.1}% ({}/{} lines) [{}]",
            cov.coverage_percent, cov.covered_lines, cov.total_lines, cov.tool);
    }
}

/// Print explicit debt markers (TODO/FIXME/HACK).
fn print_explicit_debt(items: &[void_stack_core::analyzer::explicit_debt::ExplicitDebtItem]) {
    if items.is_empty() {
        return;
    }
    use std::collections::HashMap;
    let mut by_kind: HashMap<&str, usize> = HashMap::new();
    for item in items {
        *by_kind.entry(&item.kind).or_insert(0) += 1;
    }
    let summary: Vec<String> = by_kind.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
    println!("  Explicit debt: {} markers ({})", items.len(), summary.join(", "));
    for item in items.iter().take(10) {
        let text = if item.text.len() > 50 { format!("{}...", &item.text[..47]) } else { item.text.clone() };
        println!("    [{}] {}:{} — {}", item.kind, item.file, item.line, text);
    }
    if items.len() > 10 {
        println!("    ... and {} more", items.len() - 10);
    }
}

/// Save snapshot, compare against previous if requested, and persist.
fn handle_snapshot_and_compare(
    project_path: &Path,
    named_results: &[(String, void_stack_core::analyzer::AnalysisResult)],
    label: Option<&str>,
    do_compare: bool,
    full_doc: &mut String,
) {
    use void_stack_core::analyzer::history;

    let snapshot = history::create_snapshot(named_results, label.map(|s| s.to_string()));

    if do_compare {
        print_comparison(project_path, &snapshot, full_doc);
    }

    if let Err(e) = history::save_snapshot(project_path, &snapshot) {
        eprintln!("Warning: could not save analysis snapshot: {}", e);
    }
}

/// Load the latest snapshot and print the comparison.
fn print_comparison(
    project_path: &Path,
    snapshot: &void_stack_core::analyzer::history::AnalysisSnapshot,
    full_doc: &mut String,
) {
    use void_stack_core::analyzer::history;

    if let Some(previous) = history::load_latest(project_path) {
        let comparison = history::compare(&previous, snapshot);
        let comp_md = history::comparison_markdown(&comparison);
        full_doc.push_str(&comp_md);

        println!("Debt trend: {} (vs {})",
            comparison.overall_trend,
            previous.timestamp.format("%Y-%m-%d %H:%M"));
        for svc in &comparison.services {
            println!("  {} — LOC: {}, anti-patterns: {}, complexity: {}, trend: {}",
                svc.name,
                format_delta(svc.loc_delta),
                format_delta_i32(svc.antipattern_delta),
                format_delta_f32(svc.complexity_delta),
                svc.trend);
        }
        println!();
    } else {
        println!("No previous snapshot found for comparison.\n");
    }
}

/// Run cross-project dependency analysis and append results.
fn run_cross_project_analysis(
    config: &void_stack_core::global_config::GlobalConfig,
    project: &void_stack_core::model::Project,
    named_results: &[(String, void_stack_core::analyzer::AnalysisResult)],
    full_doc: &mut String,
) {
    let mut all_analysis = HashMap::new();
    all_analysis.insert(
        project.name.clone(),
        named_results.iter().map(|(n, r)| (n.clone(), r.clone())).collect(),
    );

    for other in &config.projects {
        if other.name.eq_ignore_ascii_case(&project.name) {
            continue;
        }
        let mut other_results = Vec::new();
        for svc in &other.services {
            let dir = svc.working_dir.as_deref().unwrap_or(&other.path);
            let clean = strip_win_prefix(dir);
            if let Some(result) = void_stack_core::analyzer::analyze_project(Path::new(&clean)) {
                other_results.push((svc.name.clone(), result));
            }
        }
        if !other_results.is_empty() {
            all_analysis.insert(other.name.clone(), other_results);
        }
    }

    let cross = void_stack_core::analyzer::analyze_cross_project(&config.projects, &all_analysis);
    if !cross.links.is_empty() {
        let cross_md = void_stack_core::analyzer::cross_project::cross_project_markdown(&cross);
        full_doc.push_str(&cross_md);

        println!("Cross-project dependencies:");
        for link in &cross.links {
            println!("  {} ({}) --> {} via '{}'",
                link.from_project, link.from_service, link.to_project, link.via_dependency);
        }
        println!();
    }
}

/// Run best practices analysis and append results.
fn run_best_practices(project_path_str: &str, full_doc: &mut String) {
    use void_stack_core::analyzer::best_practices;
    use void_stack_core::analyzer::best_practices::report::generate_best_practices_markdown;

    println!("Running best practices analysis...");
    let bp_result = best_practices::analyze_best_practices(Path::new(project_path_str));

    print_best_practices_summary(&bp_result);

    let bp_md = generate_best_practices_markdown(&bp_result);
    full_doc.push_str(&bp_md);
}

/// Print the best practices summary to the console.
fn print_best_practices_summary(bp_result: &void_stack_core::analyzer::best_practices::BestPracticesResult) {
    use void_stack_core::analyzer::best_practices;

    if bp_result.tools_used.is_empty() {
        println!("  No applicable linting tools found.");
    } else {
        println!("  Overall Score: {:.0}/100", bp_result.overall_score);
        println!("  Tools: {}", bp_result.tools_used.join(", "));
        let important = bp_result.findings.iter()
            .filter(|f| f.severity == best_practices::BpSeverity::Important).count();
        let warnings = bp_result.findings.iter()
            .filter(|f| f.severity == best_practices::BpSeverity::Warning).count();
        let suggestions = bp_result.findings.iter()
            .filter(|f| f.severity == best_practices::BpSeverity::Suggestion).count();
        println!("  Findings: {} important, {} warnings, {} suggestions",
            important, warnings, suggestions);
        for ts in &bp_result.tool_scores {
            let native = ts.native_score
                .map(|n| format!(" (native: {:.0})", n))
                .unwrap_or_default();
            println!("    {} — score: {:.0}/100, {} findings{}",
                ts.tool, ts.score, ts.finding_count, native);
        }
    }
    println!();
}

/// Write the accumulated markdown to the output file.
fn save_output(full_doc: &str, output: Option<&str>, project_path: &str) -> Result<()> {
    if !full_doc.is_empty() {
        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(project_path);
                format!("{}/void-stack-analysis.md", dir)
            }
        };
        std::fs::write(&path, full_doc)?;
        println!("Analysis saved to {}", path);
    }

    Ok(())
}

// ── Formatting helpers ──────────────────────────────────────

fn format_delta(v: i64) -> String {
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
