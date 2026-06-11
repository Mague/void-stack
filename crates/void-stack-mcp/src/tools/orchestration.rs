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

    // Guard: a missing/unreadable path would make every scanner silently
    // return empty and the report would claim the project is healthy.
    if let Some(reason) = analysis_did_not_run_reason(
        project_path.is_dir(),
        1,
        true,
        &project_path.to_string_lossy(),
    ) {
        return Err(McpError::invalid_params(
            format!("analysis did not run: {}", reason),
            None,
        ));
    }

    // Step 1: check semantic index
    let has_index = void_stack_core::vector_index::index_exists(&project);

    // Step 2 + 3: audit (runs external tools) and analyze are synchronous,
    // CPU/IO-heavy work — keep them off the async runtime threads.
    let audit_name = project.name.clone();
    let audit_path = project_path.clone();
    let (audit, analysis) = tokio::task::spawn_blocking(move || {
        let audit = void_stack_core::audit::audit_project(&audit_name, &audit_path);
        let analysis = void_stack_core::analyzer::analyze_project(&audit_path);
        (audit, analysis)
    })
    .await
    .map_err(|e| McpError::internal_error(format!("analysis task failed: {}", e), None))?;

    // Guard: zero scanned files + no analyzable modules means the report
    // would be vacuously clean — surface that instead of "Risk 0/100".
    if let Some(reason) = analysis_did_not_run_reason(
        true,
        audit.scan_stats.files_scanned,
        analysis.is_some(),
        &project_path.to_string_lossy(),
    ) {
        return Err(McpError::internal_error(
            format!("analysis did not run: {}", reason),
            None,
        ));
    }

    // Step 4: identify hot spots. Complexity spots honor the project's
    // .void-audit-ignore (rule id CC-HIGH) so data-table matches and
    // template-assembly functions can be suppressed with justification.
    let spots = identify_hot_spots(&audit, analysis.as_ref());
    let (spots, cc_suppressed) = apply_cc_suppressions(spots, &project_path);

    // Step 5: enrich with semantic search (only if index exists + depth >= standard).
    // semantic_search is CPU-bound (embedding + HNSW) — spawn_blocking.
    let enriched: Vec<(usize, String)> = if has_index && depth != "quick" {
        let enrich_project = project.clone();
        let enrich_spots_in = spots.clone();
        tokio::task::spawn_blocking(move || enrich_spots(&enrich_project, &enrich_spots_in))
            .await
            .map_err(|e| McpError::internal_error(format!("enrichment task failed: {}", e), None))?
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
        cc_suppressed,
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

/// Drop Performance hot spots suppressed via `.void-audit-ignore`
/// (rule id `CC-HIGH`), returning how many were suppressed so the report's
/// `Suppressed:` counter reflects them instead of reporting 0.
fn apply_cc_suppressions(
    mut spots: Vec<HotSpot>,
    project_path: &std::path::Path,
) -> (Vec<HotSpot>, usize) {
    let before = spots.len();
    spots.retain(|s| {
        s.category != HotSpotCategory::Performance
            || !void_stack_core::audit::suppress::is_rule_suppressed(
                "CC-HIGH",
                &s.file_path,
                project_path,
            )
    });
    let suppressed = before - spots.len();
    (spots, suppressed)
}

/// Decide whether the analysis effectively did not run.
///
/// Returns `Some(reason)` when the report would be vacuously clean:
/// the path is not a directory, or nothing was scanned AND the analyzer
/// produced no result. Distinguishes "no findings" from "nothing analyzed".
fn analysis_did_not_run_reason(
    path_is_dir: bool,
    files_scanned: u32,
    analysis_present: bool,
    path_display: &str,
) -> Option<String> {
    if !path_is_dir {
        return Some(format!(
            "project path '{}' does not exist or is not a directory — \
             fix the project's registered path and retry",
            path_display
        ));
    }
    if files_scanned == 0 && !analysis_present {
        return Some(format!(
            "no scannable source files found under '{}' (0 files scanned) — \
             check that the path points at the project root and that ignore \
             rules are not excluding everything",
            path_display
        ));
    }
    None
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
    spots.sort_by_key(|s| s.severity);
    spots
}

// ── Enrichment ──────────────────────────────────────────────

/// Best-match score below which we refuse to show a snippet — a weak match
/// (CLI imports for a `lib.rs` hot spot) is worse than admitting there is
/// no representative one.
const ENRICHMENT_SCORE_FLOOR: f32 = 0.55;
const MAX_QUERY_SYMBOLS: usize = 3;

/// Build the semantic query for a hot spot. File-level spots (no symbol)
/// used to query by bare filename, which matched irrelevant chunks; now we
/// query with the file's top symbol names from the structural graph and
/// fall back to the filename only when no graph/nodes exist.
fn enrichment_query(project: &Project, spot: &HotSpot) -> String {
    if let Some(sym) = &spot.symbol {
        return format!("{} {}", sym, spot.reason);
    }
    let syms = top_file_symbols(project, &spot.file_path);
    if syms.is_empty() {
        format!("{} {}", spot.file_path, spot.reason)
    } else {
        format!("{} {}", syms.join(" "), spot.reason)
    }
}

/// Top symbol names declared in a file, from the structural graph.
/// Empty when the graph is missing or the file has no nodes.
fn top_file_symbols(project: &Project, file_path: &str) -> Vec<String> {
    let Ok(conn) = void_stack_core::structural::open_db(project) else {
        return Vec::new();
    };
    let qnames =
        void_stack_core::structural::qnames_in_files(&conn, &[file_path.replace('\\', "/")])
            .unwrap_or_default();
    let mut names: Vec<String> = qnames
        .iter()
        .filter_map(|qn| qn.rsplit("::").next())
        .filter(|n| !n.contains('/') && n.len() > 2)
        .map(|n| n.to_string())
        .collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    names.dedup();
    names.truncate(MAX_QUERY_SYMBOLS);
    names
}

fn enrich_spots(project: &Project, spots: &[HotSpot]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, spot) in spots.iter().take(MAX_ENRICHMENT_SPOTS).enumerate() {
        let query = enrichment_query(project, spot);
        if let Ok(results) = void_stack_core::vector_index::semantic_search(project, &query, 5) {
            if results.is_empty() {
                continue;
            }
            // A snippet from a DIFFERENT file is not representative of this
            // hot spot no matter how well it scores (signals.rs once showed
            // the fat-controller DETECTOR instead of its own code). Prefer
            // same-file chunks even at lower score; below the floor, admit
            // there is no representative snippet.
            let spot_file = spot.file_path.replace('\\', "/");
            let same_file: Vec<_> = results
                .iter()
                .filter(|r| {
                    let rf = r.file_path.replace('\\', "/");
                    rf == spot_file || rf.ends_with(&spot_file) || spot_file.ends_with(&rf)
                })
                .collect();
            let best_same = same_file.first().map(|r| r.score).unwrap_or(0.0);
            if best_same >= ENRICHMENT_SCORE_FLOOR {
                let snippet = same_file
                    .iter()
                    .take(2)
                    .map(|r| r.chunk.clone())
                    .collect::<Vec<_>>()
                    .join("\n---\n");
                out.push((i, snippet));
            } else {
                out.push((
                    i,
                    format!(
                        "// no representative snippet (best same-file match scored {:.2}, below the {:.2} floor)",
                        best_same, ENRICHMENT_SCORE_FLOOR
                    ),
                ));
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
    /// Performance hot spots dropped by CC-HIGH suppression rules — counted
    /// into the report's Suppressed line.
    cc_suppressed: usize,
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

/// Assemble the full report by delegating to one builder per section —
/// split from a single CC=37 function flagged as a fat controller.
fn assemble_report(ctx: &ReportCtx<'_>) -> String {
    let mut md = String::new();
    push_title(&mut md, ctx);
    push_executive_summary(&mut md, ctx);
    push_security_section(&mut md, ctx);
    push_performance_section(&mut md, ctx);
    push_architecture_section(&mut md, ctx);
    push_hotspots_section(&mut md, ctx);
    push_deep_section(&mut md, ctx);
    push_actions_section(&mut md, ctx);
    push_raw_footer(&mut md, ctx);
    md
}

fn push_title(md: &mut String, ctx: &ReportCtx<'_>) {
    let project = ctx.project;
    let audit = ctx.audit;
    let analysis = ctx.analysis;
    let elapsed = ctx.elapsed;
    let depth = ctx.depth;
    let has_index = ctx.has_index;
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
}

fn push_executive_summary(md: &mut String, ctx: &ReportCtx<'_>) {
    let audit = ctx.audit;
    let analysis = ctx.analysis;
    let spots = ctx.spots;
    // Executive summary
    md.push_str("## Executive Summary\n\n");
    md.push_str(&format!(
        "- **Risk:** {:.0}/100 | Findings: Critical:{} High:{} Medium:{} Low:{}{}\n",
        audit.summary.risk_score,
        audit.summary.critical,
        audit.summary.high,
        audit.summary.medium,
        audit.summary.low,
        if audit.suppressed > 0 || ctx.cc_suppressed > 0 {
            format!(
                " | Suppressed: {}",
                audit.suppressed as usize + ctx.cc_suppressed
            )
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
}

fn push_security_section(md: &mut String, ctx: &ReportCtx<'_>) {
    let audit = ctx.audit;
    let focus = ctx.focus;
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
                    .filter(|f| f.adjusted_severity == sev)
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
                .filter(|f| f.adjusted_severity == S::Medium)
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
                .filter(|f| f.adjusted_severity == S::Info)
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
                sorted.sort_by_key(|x| std::cmp::Reverse(x.1));
                for (reason, count) in &sorted {
                    md.push_str(&format!("- {}x {}\n", count, reason));
                }
                md.push_str("\n</details>\n\n");
            }
        }
    }
}

fn push_performance_section(md: &mut String, ctx: &ReportCtx<'_>) {
    let spots = ctx.spots;
    let focus = ctx.focus;
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
}

fn push_architecture_section(md: &mut String, ctx: &ReportCtx<'_>) {
    let spots = ctx.spots;
    let focus = ctx.focus;
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
}

fn push_hotspots_section(md: &mut String, ctx: &ReportCtx<'_>) {
    let spots = ctx.spots;
    let enriched = ctx.enriched;
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
}

fn push_deep_section(md: &mut String, ctx: &ReportCtx<'_>) {
    let deep = ctx.deep;
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
}

fn push_actions_section(md: &mut String, ctx: &ReportCtx<'_>) {
    let audit = ctx.audit;
    let spots = ctx.spots;
    // Recommended actions
    md.push_str("## Recommended Next Actions\n\n");
    let actions = generate_actions(audit, spots);
    for (i, a) in actions.iter().enumerate() {
        md.push_str(&format!("{}. {}\n", i + 1, a));
    }
    md.push('\n');
}

fn push_raw_footer(md: &mut String, ctx: &ReportCtx<'_>) {
    let audit = ctx.audit;
    let analysis = ctx.analysis;
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

    // Scan stats footer — proves the analysis actually ran.
    let stats = &audit.scan_stats;
    let phases = stats
        .phase_timings
        .iter()
        .map(|(name, ms)| format!("{} {}ms", name, ms))
        .collect::<Vec<_>>()
        .join(", ");
    md.push_str(&format!(
        "\n---\n*Scan stats: {} files scanned | {} rules executed | analyzer modules: {} | phases: {}*\n",
        stats.files_scanned,
        stats.rules_executed,
        analysis.map(|a| a.graph.modules.len()).unwrap_or(0),
        phases,
    ));
    if stats.files_scanned == 0 {
        md.push_str(
            "\n⚠️ **Warning: the audit scanned 0 files — findings above do not reflect project health.**\n",
        );
    }
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
    sorted.sort_by_key(|x| std::cmp::Reverse(x.1));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(file: &str, symbol: Option<&str>) -> HotSpot {
        HotSpot {
            file_path: file.to_string(),
            symbol: symbol.map(|s| s.to_string()),
            reason: "CC=42".to_string(),
            category: HotSpotCategory::Performance,
            severity: Severity::Medium,
            language: "rust".to_string(),
        }
    }

    /// A CC finding suppressed via .void-audit-ignore must increment the
    /// suppressed counter that feeds the report's 'Suppressed:' display —
    /// previously it was dropped silently and the report said 0.
    #[test]
    fn test_cc_suppression_increments_counter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".void-audit-ignore"),
            "# data table\nCC-HIGH crates/tui/src/i18n.rs\n",
        )
        .unwrap();

        let spots = vec![
            spot("crates/tui/src/i18n.rs", None),
            spot("crates/core/src/real_hotspot.rs", None),
        ];
        let (kept, suppressed) = apply_cc_suppressions(spots, dir.path());
        assert_eq!(suppressed, 1, "suppressed CC spot must be counted");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].file_path, "crates/core/src/real_hotspot.rs");
    }

    #[test]
    fn test_enrichment_query_prefers_structural_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: format!("enrich-fixture-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let conn = void_stack_core::structural::open_db(&project).unwrap();
        let node = |name: &str| void_stack_core::structural::StructuralNode {
            kind: void_stack_core::structural::NodeKind::Function,
            name: name.to_string(),
            qualified_name: format!("src/lib.rs::{}", name),
            file_path: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 5,
            language: "rust".to_string(),
            parent_name: None,
            is_test: false,
        };
        void_stack_core::structural::store_file(
            &conn,
            "src/lib.rs",
            &[node("start_indexing"), node("resolve_symbols")],
            &[],
            "h",
        )
        .unwrap();

        let q = enrichment_query(&project, &spot("src/lib.rs", None));
        assert!(
            q.contains("start_indexing") || q.contains("resolve_symbols"),
            "file-level spots must query by the file's symbols, got: {q}"
        );
        assert!(!q.starts_with("src/lib.rs"), "got: {q}");
    }

    #[test]
    fn test_enrichment_query_falls_back_to_filename_without_graph() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: format!("enrich-nograph-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let q = enrichment_query(&project, &spot("src/server.rs", None));
        assert!(q.contains("src/server.rs"), "got: {q}");
    }

    #[test]
    fn test_enrichment_query_symbol_spots_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: "irrelevant".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let q = enrichment_query(&project, &spot("src/x.rs", Some("handle_request")));
        assert!(q.starts_with("handle_request"), "got: {q}");
    }

    #[test]
    fn test_did_not_run_when_path_missing() {
        let reason = analysis_did_not_run_reason(false, 0, false, "/no/such/path");
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("/no/such/path"));
    }

    #[test]
    fn test_did_not_run_when_nothing_scanned() {
        let reason = analysis_did_not_run_reason(true, 0, false, "/empty");
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("0 files scanned"));
    }

    #[test]
    fn test_runs_when_files_scanned() {
        assert!(analysis_did_not_run_reason(true, 42, true, "/proj").is_none());
    }

    #[test]
    fn test_runs_when_only_analyzer_found_modules() {
        // Audit scanned nothing but the analyzer built a graph — report runs
        // (the footer still shows files_scanned == 0 with a warning).
        assert!(analysis_did_not_run_reason(true, 0, true, "/proj").is_none());
    }
}
