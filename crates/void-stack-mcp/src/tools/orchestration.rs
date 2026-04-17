//! `full_analysis` MCP tool — combines audit, architecture analysis, and
//! semantic hot-spot detection into a single structured markdown report.

use std::collections::HashMap;

use rmcp::ErrorData as McpError;
use rmcp::model::*;

use crate::server::VoidStackMcp;
use crate::types::FullAnalysisRequest;

use void_stack_core::audit::findings::{AuditResult, Severity};
use void_stack_core::model::Project;
use void_stack_core::runner::local::strip_win_prefix;

// ── Thresholds (named constants, easy to tune) ──────────────

const CC_HIGH: usize = 30;
const FINDINGS_PER_FILE_THRESHOLD: usize = 3;
const MAX_ENRICHMENT_SPOTS: usize = 8;
const MAX_DEEP_FILES: usize = 3;
const MAX_FILE_LINES: usize = 80;

// ���─ HotSpot model ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum HotSpotCategory {
    Security,
    Performance,
    Architecture,
}

#[derive(Debug, Clone)]
struct HotSpot {
    file_path: String,
    symbol: Option<String>,
    reason: String,
    category: HotSpotCategory,
    severity: Severity,
    language: String,
}

fn detect_language(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" => "rust",
        "py" => "python",
        "go" => "go",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "dart" => "dart",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" => "cpp",
        "cs" => "csharp",
        "vue" => "vue",
        "svelte" => "svelte",
        _ => "unknown",
    }
}

// ── Main handler ────────────────────────────────────────────

pub async fn full_analysis(
    _mcp: &VoidStackMcp,
    req: FullAnalysisRequest,
) -> Result<CallToolResult, McpError> {
    let start = std::time::Instant::now();
    let depth = req.depth.as_deref().unwrap_or("standard");
    let focus = req.focus.unwrap_or_else(|| {
        vec![
            "security".into(),
            "performance".into(),
            "architecture".into(),
        ]
    });

    let config = VoidStackMcp::load_config()?;
    let project = VoidStackMcp::find_project_or_err(&config, &req.project)?;
    let project_path = std::path::PathBuf::from(strip_win_prefix(&project.path));

    // Step 1: check semantic index
    let has_index = void_stack_core::vector_index::index_exists(&project);

    // Step 2: audit (sync — runs external tools, may take a few seconds)
    let audit = void_stack_core::audit::audit_project(&project.name, &project_path);

    // Step 3: analyze (sync)
    let analysis = void_stack_core::analyzer::analyze_project(&project_path);

    // Step 4: identify hot spots
    let spots = identify_hot_spots(&audit, analysis.as_ref());

    // Step 5: enrich with semantic search (only if index exists + depth >= standard)
    let enriched: Vec<(usize, String)> = if has_index && depth != "quick" {
        enrich_spots(&project, &spots)
    } else {
        Vec::new()
    };

    // Step 6: deep file reads (only if depth == "deep" and index exists)
    let deep_context: Vec<(String, String)> = if depth == "deep" {
        read_top_files(&project_path, &spots)
    } else {
        Vec::new()
    };

    // Step 7: assemble report
    let report = assemble_report(&ReportCtx {
        project: &project,
        audit: &audit,
        analysis: analysis.as_ref(),
        spots: &spots,
        enriched: &enriched,
        deep: &deep_context,
        focus: &focus,
        elapsed: start.elapsed(),
        depth,
        has_index,
    });

    Ok(CallToolResult::success(vec![Content::text(report)]))
}

// ── Hot spot identification ─────────────────────────────────

fn identify_hot_spots(
    audit: &AuditResult,
    analysis: Option<&void_stack_core::analyzer::AnalysisResult>,
) -> Vec<HotSpot> {
    let mut spots = Vec::new();

    // Complex functions (from analysis)
    if let Some(a) = analysis {
        if let Some(ref complexity) = a.complexity {
            for (file, fc) in complexity {
                for func in &fc.functions {
                    if func.complexity >= CC_HIGH {
                        spots.push(HotSpot {
                            file_path: file.clone(),
                            symbol: Some(func.name.clone()),
                            reason: format!("CC={}", func.complexity),
                            category: HotSpotCategory::Performance,
                            severity: if func.complexity > 40 {
                                Severity::High
                            } else {
                                Severity::Medium
                            },
                            language: detect_language(file).to_string(),
                        });
                    }
                }
            }
        }

        // Anti-patterns (High severity only)
        for ap in &a.architecture.anti_patterns {
            if matches!(
                ap.severity,
                void_stack_core::analyzer::patterns::antipatterns::Severity::High
            ) {
                for module in &ap.affected_modules {
                    spots.push(HotSpot {
                        file_path: module.clone(),
                        symbol: None,
                        reason: format!("{}", ap.kind),
                        category: HotSpotCategory::Architecture,
                        severity: Severity::High,
                        language: detect_language(module).to_string(),
                    });
                }
            }
        }
    }

    // Files with many audit findings
    let mut by_file: HashMap<String, usize> = HashMap::new();
    for f in &audit.findings {
        if let Some(ref fp) = f.file_path {
            *by_file.entry(fp.clone()).or_default() += 1;
        }
    }
    for (file, count) in &by_file {
        if *count > FINDINGS_PER_FILE_THRESHOLD {
            spots.push(HotSpot {
                file_path: file.clone(),
                symbol: None,
                reason: format!("{} findings", count),
                category: HotSpotCategory::Security,
                severity: Severity::Medium,
                language: detect_language(file).to_string(),
            });
        }
    }

    // Dedupe + sort by severity
    spots.sort_by(|a, b| (&a.file_path, &a.symbol).cmp(&(&b.file_path, &b.symbol)));
    spots.dedup_by(|a, b| a.file_path == b.file_path && a.symbol == b.symbol);
    spots.sort_by(|a, b| a.severity.cmp(&b.severity));
    spots
}

// ── Enrichment ──────────────────────────────────────────────

fn enrich_spots(project: &Project, spots: &[HotSpot]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, spot) in spots.iter().take(MAX_ENRICHMENT_SPOTS).enumerate() {
        let query = match &spot.symbol {
            Some(sym) => format!("{} {}", sym, spot.reason),
            None => format!("{} {}", spot.file_path, spot.reason),
        };
        if let Ok(results) = void_stack_core::vector_index::semantic_search(project, &query, 2) {
            let snippet = results
                .iter()
                .map(|r| r.chunk.clone())
                .collect::<Vec<_>>()
                .join("\n---\n");
            if !snippet.is_empty() {
                out.push((i, snippet));
            }
        }
    }
    out
}

fn read_top_files(project_path: &std::path::Path, spots: &[HotSpot]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for spot in spots.iter().take(MAX_DEEP_FILES) {
        let abs = project_path.join(&spot.file_path);
        if let Ok(content) = std::fs::read_to_string(&abs) {
            let truncated: String = content
                .lines()
                .take(MAX_FILE_LINES)
                .collect::<Vec<_>>()
                .join("\n");
            out.push((spot.file_path.clone(), truncated));
        }
    }
    out
}

// ── Report assembly ─────────────────────────────────────────

struct ReportCtx<'a> {
    project: &'a Project,
    audit: &'a AuditResult,
    analysis: Option<&'a void_stack_core::analyzer::AnalysisResult>,
    spots: &'a [HotSpot],
    enriched: &'a [(usize, String)],
    deep: &'a [(String, String)],
    focus: &'a [String],
    elapsed: std::time::Duration,
    depth: &'a str,
    has_index: bool,
}

fn assemble_report(ctx: &ReportCtx<'_>) -> String {
    let project = ctx.project;
    let audit = ctx.audit;
    let analysis = ctx.analysis;
    let spots = ctx.spots;
    let enriched = ctx.enriched;
    let deep = ctx.deep;
    let focus = ctx.focus;
    let elapsed = ctx.elapsed;
    let depth = ctx.depth;
    let has_index = ctx.has_index;
    let mut md = String::new();

    // Title + language mix
    md.push_str(&format!(
        "# Full Analysis — {}\n*Generated: {} | Duration: {}ms | Depth: {}",
        project.name,
        audit.timestamp,
        elapsed.as_millis(),
        depth,
    ));
    if let Some(a) = analysis {
        let mix = language_mix(&a.graph.modules);
        if !mix.is_empty() {
            md.push_str(&format!(" | Language mix: {}", mix));
        }
    }
    if !has_index && depth != "quick" {
        md.push_str(" | Semantic search unavailable — run `void index` for deeper analysis");
    }
    md.push_str("*\n\n");

    // Executive summary
    md.push_str("## Executive Summary\n\n");
    md.push_str(&format!(
        "- **Risk:** {:.0}/100 | Findings: Critical:{} High:{} Medium:{} Low:{}{}\n",
        audit.summary.risk_score,
        audit.summary.critical,
        audit.summary.high,
        audit.summary.medium,
        audit.summary.low,
        if audit.suppressed > 0 {
            format!(" | Suppressed: {}", audit.suppressed)
        } else {
            String::new()
        },
    ));
    if let Some(a) = analysis {
        md.push_str(&format!(
            "- **Architecture:** {} ({} modules, {} LOC, {} deps)\n",
            a.architecture.detected_pattern,
            a.graph.modules.len(),
            a.graph.modules.iter().map(|m| m.loc).sum::<usize>(),
            a.graph.external_deps.len(),
        ));
        md.push_str(&format!(
            "- **Anti-patterns:** {}\n",
            a.architecture.anti_patterns.len(),
        ));
    }
    if let Some(top) = spots.first() {
        md.push_str(&format!(
            "- **Top concern:** {} at `{}` ({})\n",
            top.reason, top.file_path, top.language,
        ));
    }
    md.push('\n');

    // Security section — uses adjusted_severity for classification
    if focus.iter().any(|f| f == "security") {
        md.push_str("## Security\n\n");
        if audit.findings.is_empty() {
            md.push_str("No findings.\n\n");
        } else {
            use void_stack_core::audit::findings::Severity as S;
            // Critical/High: always visible in full
            for sev in [S::Critical, S::High] {
                let list: Vec<_> = audit
                    .findings
                    .iter()
                    .filter(|f| f.adjusted_severity.unwrap_or(f.severity) == sev)
                    .collect();
                if !list.is_empty() {
                    md.push_str(&format!("### {} ({} findings)\n", sev, list.len()));
                    for f in &list {
                        md.push_str(&format!(
                            "- **{}** — `{}`:{}\n  -> {}\n",
                            f.title,
                            f.file_path.as_deref().unwrap_or("n/a"),
                            f.line_number.unwrap_or(0),
                            f.remediation,
                        ));
                    }
                    md.push('\n');
                }
            }
            // Medium: up to 10 detailed
            let mediums: Vec<_> = audit
                .findings
                .iter()
                .filter(|f| f.adjusted_severity.unwrap_or(f.severity) == S::Medium)
                .collect();
            if !mediums.is_empty() {
                md.push_str(&format!("### Medium ({} findings)\n", mediums.len()));
                for f in mediums.iter().take(10) {
                    md.push_str(&format!(
                        "- `{}`:{} — {}\n",
                        f.file_path.as_deref().unwrap_or("n/a"),
                        f.line_number.unwrap_or(0),
                        f.title,
                    ));
                }
                if mediums.len() > 10 {
                    md.push_str(&format!(
                        "\n*...and {} more Medium findings*\n",
                        mediums.len() - 10
                    ));
                }
                md.push('\n');
            }
            // Info: collapsible, grouped by adjustment_reason
            let infos: Vec<_> = audit
                .findings
                .iter()
                .filter(|f| f.adjusted_severity.unwrap_or(f.severity) == S::Info)
                .collect();
            if !infos.is_empty() {
                md.push_str(&format!(
                    "<details><summary>{} findings downgraded to Info</summary>\n\n",
                    infos.len()
                ));
                let mut reason_counts: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for f in &infos {
                    let reason = f
                        .adjustment_reason
                        .clone()
                        .unwrap_or_else(|| "No reason".into());
                    *reason_counts.entry(reason).or_default() += 1;
                }
                let mut sorted: Vec<_> = reason_counts.into_iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(&a.1));
                for (reason, count) in &sorted {
                    md.push_str(&format!("- {}x {}\n", count, reason));
                }
                md.push_str("\n</details>\n\n");
            }
        }
    }

    // Performance section
    if focus.iter().any(|f| f == "performance") {
        md.push_str("## Performance (Complexity)\n\n");
        let perf_spots: Vec<_> = spots
            .iter()
            .filter(|s| s.category == HotSpotCategory::Performance)
            .take(10)
            .collect();
        if perf_spots.is_empty() {
            md.push_str("No high-complexity functions found.\n\n");
        } else {
            for s in &perf_spots {
                md.push_str(&format!(
                    "- **{}** `{}::{}` ({})\n",
                    s.reason,
                    s.file_path,
                    s.symbol.as_deref().unwrap_or("?"),
                    s.language,
                ));
            }
            md.push('\n');
        }
    }

    // Architecture section
    if focus.iter().any(|f| f == "architecture") {
        md.push_str("## Architecture\n\n");
        let arch_spots: Vec<_> = spots
            .iter()
            .filter(|s| s.category == HotSpotCategory::Architecture)
            .take(10)
            .collect();
        if arch_spots.is_empty() {
            md.push_str("No High-severity anti-patterns.\n\n");
        } else {
            for s in &arch_spots {
                md.push_str(&format!(
                    "- **{}** `{}` ({})\n",
                    s.reason, s.file_path, s.language,
                ));
            }
            md.push('\n');
        }
    }

    // Hot spots with enrichment
    if !enriched.is_empty() {
        md.push_str("## Hot Spots (semantic context)\n\n");
        for (idx, snippet) in enriched.iter().take(5) {
            if let Some(spot) = spots.get(*idx) {
                md.push_str(&format!(
                    "### `{}`{}\n- Language: {}\n- Why flagged: {}\n- Suggested action: {}\n\n```{}\n{}\n```\n\n",
                    spot.file_path,
                    spot.symbol.as_deref().map(|s| format!("::{}", s)).unwrap_or_default(),
                    spot.language,
                    spot.reason,
                    suggest_action(spot),
                    spot.language,
                    truncate_snippet(snippet, 30),
                ));
            }
        }
    }

    // Deep file context
    if !deep.is_empty() {
        md.push_str("## Deep Dive (file context)\n\n");
        for (path, content) in deep {
            let lang = detect_language(path);
            md.push_str(&format!(
                "### `{}`\n```{}\n{}\n```\n\n",
                path, lang, content
            ));
        }
    }

    // Recommended actions
    md.push_str("## Recommended Next Actions\n\n");
    let actions = generate_actions(audit, spots);
    for (i, a) in actions.iter().enumerate() {
        md.push_str(&format!("{}. {}\n", i + 1, a));
    }
    md.push('\n');

    // Raw data (collapsible)
    md.push_str("<details><summary>Raw data</summary>\n\n");
    md.push_str(&format!(
        "```\nRisk: {:.0}/100 | Critical:{} High:{} Medium:{} Low:{} Info:{}\nTotal findings: {}\nSuppressed: {}\n```\n",
        audit.summary.risk_score,
        audit.summary.critical, audit.summary.high,
        audit.summary.medium, audit.summary.low, audit.summary.info,
        audit.summary.total, audit.suppressed,
    ));
    md.push_str("</details>\n");

    md
}

fn suggest_action(spot: &HotSpot) -> &'static str {
    match (&spot.category, &spot.severity) {
        (HotSpotCategory::Performance, Severity::High | Severity::Critical) => {
            "Refactor: extract helpers to reduce cyclomatic complexity"
        }
        (HotSpotCategory::Security, _) => "Review findings and apply remediations",
        (HotSpotCategory::Architecture, _) => "Split responsibilities into focused modules",
        _ => "Review manually",
    }
}

fn generate_actions(audit: &AuditResult, spots: &[HotSpot]) -> Vec<String> {
    let mut actions = Vec::new();

    // Critical/High always surface
    if audit.summary.critical > 0 {
        actions.push(format!(
            "Address {} Critical security findings immediately",
            audit.summary.critical
        ));
    }
    if audit.summary.high > 0 {
        actions.push(format!(
            "Review {} High severity findings this week",
            audit.summary.high
        ));
    }

    // Medium: differentiated thresholds
    if audit.summary.medium >= 20 {
        actions.push(format!(
            "High Medium count ({}) — plan a dedicated cleanup sprint",
            audit.summary.medium
        ));
    } else if audit.summary.medium >= 5 {
        actions.push(format!(
            "Review {} Medium findings during regular iteration",
            audit.summary.medium
        ));
    }

    // Performance hot spots (top 3)
    for spot in spots
        .iter()
        .filter(|s| s.category == HotSpotCategory::Performance)
        .take(3)
    {
        actions.push(format!(
            "Refactor `{}` ({}) — {}",
            spot.symbol.as_deref().unwrap_or(&spot.file_path),
            spot.language,
            spot.reason,
        ));
    }

    // Architecture: fat controllers
    let fat_count = spots
        .iter()
        .filter(|s| s.category == HotSpotCategory::Architecture)
        .count();
    if fat_count >= 3 {
        actions.push(format!(
            "{} architecture anti-patterns detected — consider splitting responsibilities",
            fat_count
        ));
    }

    // Fallback: truly healthy
    if actions.is_empty() {
        actions.push("Project looks healthy — consider adding tests or documentation.".into());
    }
    actions
}

fn language_mix(modules: &[void_stack_core::analyzer::graph::ModuleNode]) -> String {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for m in modules {
        let lang = detect_language(&m.path);
        if lang != "unknown" {
            *counts.entry(lang).or_default() += 1;
        }
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted
        .iter()
        .take(3)
        .map(|(lang, n)| format!("{} ({})", lang, n))
        .collect::<Vec<_>>()
        .join(", ")
}

fn truncate_snippet(s: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= max_lines {
        return s.to_string();
    }
    let mut out: String = lines[..max_lines].join("\n");
    out.push_str(&format!(
        "\n// ... ({} more lines)",
        lines.len() - max_lines
    ));
    out
}
