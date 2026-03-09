use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};

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
    use void_stack_core::runner::local::strip_win_prefix;
    use void_stack_core::analyzer::history;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    // Collect directories to analyze
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

    let mut full_doc = String::new();
    let mut named_results: Vec<(String, void_stack_core::analyzer::AnalysisResult)> = Vec::new();
    let project_path_str = strip_win_prefix(&project.path);

    if bp_only {
        // Skip architecture analysis — go straight to best practices
    } else {

    for (svc_name, dir) in &dirs {
        println!("Analyzing {}...", svc_name);

        match void_stack_core::analyzer::analyze_project(dir) {
            Some(result) => {
                let doc = void_stack_core::analyzer::generate_docs(&result, svc_name);
                full_doc.push_str(&doc);
                full_doc.push_str("\n\n---\n\n");

                // Print summary to console
                println!("  Pattern: {} ({:.0}% confidence)", result.architecture.detected_pattern, result.architecture.confidence * 100.0);
                println!("  Modules: {}", result.graph.modules.len());
                let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();
                println!("  LOC: {}", total_loc);
                println!("  External deps: {}", result.graph.external_deps.len());

                // Complexity summary
                if let Some(cx) = &result.complexity {
                    let all_funcs: Vec<_> = cx.iter()
                        .flat_map(|(_, fc)| fc.functions.iter())
                        .collect();
                    if !all_funcs.is_empty() {
                        let max = all_funcs.iter().max_by_key(|f| f.complexity).unwrap();
                        let complex_count = all_funcs.iter().filter(|f| f.complexity >= 10).count();
                        println!("  Complexity: max {} ({}), {} complex functions",
                            max.complexity, max.name, complex_count);
                    }
                }

                if !result.architecture.anti_patterns.is_empty() {
                    println!("  Anti-patterns: {}", result.architecture.anti_patterns.len());
                    for ap in &result.architecture.anti_patterns {
                        println!("    [{:?}] {}: {}", ap.severity, ap.kind, ap.description);
                    }
                } else {
                    println!("  No anti-patterns detected.");
                }
                if let Some(cov) = &result.coverage {
                    println!("  Coverage: {:.1}% ({}/{} lines) [{}]",
                        cov.coverage_percent, cov.covered_lines, cov.total_lines, cov.tool);
                }
                println!();

                named_results.push((svc_name.clone(), result));
            }
            None => {
                println!("  Could not detect language for {}", dir.display());
            }
        }
    }

    // Save snapshot for debt tracking
    let project_path = Path::new(&project_path_str);
    if !named_results.is_empty() {
        let snapshot = history::create_snapshot(&named_results, label.map(|s| s.to_string()));

        // Compare against previous if requested
        if do_compare {
            if let Some(previous) = history::load_latest(project_path) {
                let comparison = history::compare(&previous, &snapshot);
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

        // Save current snapshot
        if let Err(e) = history::save_snapshot(project_path, &snapshot) {
            eprintln!("Warning: could not save analysis snapshot: {}", e);
        }
    }

    // Cross-project analysis
    if do_cross_project && !named_results.is_empty() {
        let mut all_analysis = HashMap::new();
        all_analysis.insert(project.name.clone(), named_results.iter().map(|(n, r)| (n.clone(), r.clone())).collect());

        // Analyze other projects too for cross-referencing
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

    } // end if !bp_only

    // Best practices analysis
    if do_best_practices {
        use void_stack_core::analyzer::best_practices;
        use void_stack_core::analyzer::best_practices::report::generate_best_practices_markdown;

        println!("Running best practices analysis...");
        let bp_result = best_practices::analyze_best_practices(Path::new(&project_path_str));

        // Print summary
        if bp_result.tools_used.is_empty() {
            println!("  No applicable linting tools found.");
        } else {
            println!("  Overall Score: {:.0}/100", bp_result.overall_score);
            println!("  Tools: {}", bp_result.tools_used.join(", "));
            let important = bp_result.findings.iter().filter(|f| f.severity == best_practices::BpSeverity::Important).count();
            let warnings = bp_result.findings.iter().filter(|f| f.severity == best_practices::BpSeverity::Warning).count();
            let suggestions = bp_result.findings.iter().filter(|f| f.severity == best_practices::BpSeverity::Suggestion).count();
            println!("  Findings: {} important, {} warnings, {} suggestions", important, warnings, suggestions);
            for ts in &bp_result.tool_scores {
                let native = ts.native_score.map(|n| format!(" (native: {:.0})", n)).unwrap_or_default();
                println!("    {} — score: {:.0}/100, {} findings{}", ts.tool, ts.score, ts.finding_count, native);
            }
        }
        println!();

        let bp_md = generate_best_practices_markdown(&bp_result);
        full_doc.push_str(&bp_md);
    }

    if !full_doc.is_empty() {
        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(&project.path);
                format!("{}/void-stack-analysis.md", dir)
            }
        };
        std::fs::write(&path, &full_doc)?;
        println!("Analysis saved to {}", path);
    }

    Ok(())
}

// ── Diagram ──────────────────────────────────────────────────

pub fn cmd_diagram(project_name: &str, output: Option<&str>, format: &str) -> Result<()> {
    use void_stack_core::runner::local::strip_win_prefix;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let is_drawio = format.eq_ignore_ascii_case("drawio") || format.eq_ignore_ascii_case("draw.io");

    if is_drawio {
        let content = void_stack_core::diagram::drawio::generate_all(project);
        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(&project.path);
                format!("{}/void-stack-diagrams.drawio", dir)
            }
        };
        std::fs::write(&path, &content)?;
        println!("Draw.io diagram saved to {}", path);
    } else {
        // Mermaid format
        let diagrams = void_stack_core::diagram::generate_all(project);
        let mut content = String::new();
        content.push_str(&format!("# {} — Architecture\n\n", project.name));
        content.push_str("## Service Architecture\n\n");
        content.push_str(&diagrams.architecture);
        content.push_str("\n\n");

        if let Some(api) = &diagrams.api_routes {
            content.push_str("## API Routes\n\n");
            content.push_str(api);
            content.push_str("\n\n");
        }

        if let Some(db) = &diagrams.db_models {
            content.push_str("## Database Models\n\n");
            content.push_str(db);
            content.push_str("\n\n");
        }

        if !diagrams.warnings.is_empty() {
            content.push_str("## Advertencias\n\n");
            for w in &diagrams.warnings {
                content.push_str(&format!("- {}\n", w));
            }
            content.push_str("\n");

            for w in &diagrams.warnings {
                println!("  Warning: {}", w);
            }
        }

        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(&project.path);
                format!("{}/void-stack-diagrams.md", dir)
            }
        };
        std::fs::write(&path, &content)?;
        println!("Mermaid diagrams saved to {}", path);
    }

    Ok(())
}

// ── Security Audit ──────────────────────────────────────────

pub fn cmd_audit(project_name: &str, output: Option<&str>) -> Result<()> {
    use void_stack_core::audit;
    use void_stack_core::runner::local::strip_win_prefix;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let clean_path = strip_win_prefix(&project.path);
    println!("Running security audit for '{}'...\n", project.name);

    let result = audit::audit_project(&project.name, Path::new(&clean_path));

    // Print summary
    if result.summary.total == 0 {
        println!("  ✅ No se encontraron problemas de seguridad.\n");
    } else {
        println!("  Hallazgos:");
        if result.summary.critical > 0 {
            println!("    🔴 Critical: {}", result.summary.critical);
        }
        if result.summary.high > 0 {
            println!("    🟠 High:     {}", result.summary.high);
        }
        if result.summary.medium > 0 {
            println!("    🟡 Medium:   {}", result.summary.medium);
        }
        if result.summary.low > 0 {
            println!("    🔵 Low:      {}", result.summary.low);
        }
        if result.summary.info > 0 {
            println!("    ℹ️  Info:     {}", result.summary.info);
        }
        println!("    Total:       {}", result.summary.total);
        println!("    Risk Score:  {:.0}/100\n", result.summary.risk_score);

        // Print findings
        for finding in &result.findings {
            let icon = match finding.severity {
                audit::Severity::Critical => "🔴",
                audit::Severity::High => "🟠",
                audit::Severity::Medium => "🟡",
                audit::Severity::Low => "🔵",
                audit::Severity::Info => "ℹ️",
            };
            println!("  {} [{}] {}", icon, finding.severity, finding.title);
            println!("     {}", finding.description);
            if let Some(ref path) = finding.file_path {
                if let Some(line) = finding.line_number {
                    println!("     Archivo: {}:{}", path, line);
                } else {
                    println!("     Archivo: {}", path);
                }
            }
            println!("     Fix: {}", finding.remediation);
            println!();
        }
    }

    // Save report
    let report = audit::generate_report(&result);
    let path = match output {
        Some(p) => p.to_string(),
        None => format!("{}/void-stack-audit.md", clean_path),
    };
    std::fs::write(&path, &report)?;
    println!("Audit report saved to {}", path);

    Ok(())
}

// ── AI Suggestions ──────────────────────────────────────────

pub async fn cmd_suggest(project_name: &str, model_override: Option<&str>, service_filter: Option<&str>, raw: bool) -> Result<()> {
    use void_stack_core::ai;
    use void_stack_core::runner::local::strip_win_prefix;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    // Load AI config
    let mut ai_config = ai::load_ai_config().unwrap_or_default();
    if let Some(model) = model_override {
        ai_config.model = model.to_string();
    }

    println!("Analizando proyecto '{}'...\n", project.name);

    // Collect analysis results
    let services: Vec<_> = match service_filter {
        Some(svc_name) => {
            project.services.iter()
                .filter(|s| s.name.eq_ignore_ascii_case(svc_name))
                .collect()
        }
        None => project.services.iter().collect(),
    };

    let mut analysis = None;
    for svc in &services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let clean = strip_win_prefix(dir);
        let path = std::path::Path::new(&clean);
        if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
            analysis = Some(result);
            break; // Use first analyzable service
        }
    }

    let analysis = analysis
        .ok_or_else(|| anyhow::anyhow!("No se pudo analizar el proyecto (sin archivos fuente detectados)"))?;

    println!("Generando sugerencias con {} ({})...\n", ai_config.provider_name(), ai_config.model);

    match ai::suggest(&ai_config, &analysis, &project.name).await {
        Ok(result) => {
            if raw {
                println!("{}", result.raw_response);
            } else {
                println!("Modelo: {}\n", result.model_used);
                if result.suggestions.is_empty() {
                    println!("  No se generaron sugerencias estructuradas.");
                    println!("\nRespuesta completa:\n{}", result.raw_response);
                } else {
                    for (i, s) in result.suggestions.iter().enumerate() {
                        let priority_icon = match s.priority {
                            ai::SuggestionPriority::Critical => "!!",
                            ai::SuggestionPriority::High => "! ",
                            ai::SuggestionPriority::Medium => "- ",
                            ai::SuggestionPriority::Low => "  ",
                        };
                        println!("{}. {} [{}] {}", i + 1, priority_icon, s.category, s.title);
                        println!("   {}", s.description);
                        if !s.affected_files.is_empty() {
                            println!("   Archivos: {}", s.affected_files.join(", "));
                        }
                        println!();
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error de AI: {}\n", e);
            println!("Mostrando contexto de análisis que puedes usar con tu asistente AI:\n");
            let context = ai::build_context(&analysis, &project.name);
            println!("{}", context);
        }
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
