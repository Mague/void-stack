//! `void briefing`: consolidated daily report for the active projects.
//!
//! One markdown document covering, per active project: service inventory,
//! debt trend vs the previous analysis snapshot, NEW audit findings since
//! the last briefing (delta state kept under `<global dir>/briefings/state/`),
//! dead-code count, and the Doing/Review tasks from BOARD.md. Printed to
//! stdout and optionally saved to `<global dir>/briefings/YYYY-MM-DD.md`;
//! the daemon can run it on a daily schedule (see `BriefingConfig`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::global_config::GlobalConfig;
use crate::model::Project;
use crate::runner::local::strip_win_prefix;

const MAX_NEW_FINDINGS: usize = 8;
const MAX_BOARD_TASKS: usize = 10;

/// `<data_local_dir>/void-stack/briefings`.
pub fn briefings_dir() -> Result<PathBuf, String> {
    let dir = crate::global_config::global_config_dir()
        .map_err(|e| e.to_string())?
        .join("briefings");
    Ok(dir)
}

/// Generate the briefing for the config's active projects; `only`
/// overrides the active list (useful for ad-hoc runs on any project).
pub fn generate_briefing(config: &GlobalConfig, only: Option<&[String]>) -> Result<String, String> {
    let names: Vec<String> = match only {
        Some(list) if !list.is_empty() => list.to_vec(),
        _ => config.briefing.active_projects.clone(),
    };
    if names.is_empty() {
        return Err(
            "no active projects — add some with `void briefing active <project> on` \
             (or pass --project)"
                .to_string(),
        );
    }

    let today = chrono::Local::now().format("%Y-%m-%d");
    let mut md = format!("# Daily briefing — {}\n", today);

    for name in &names {
        let Some(project) = crate::global_config::find_project(config, name) else {
            md.push_str(&format!("\n## {}\n- not found in the registry\n", name));
            continue;
        };
        md.push_str(&project_section(config, project));
    }
    Ok(md)
}

/// Save the briefing as `briefings/YYYY-MM-DD.md`; returns the path.
pub fn save_briefing(markdown: &str, date: chrono::NaiveDate) -> Result<PathBuf, String> {
    let dir = briefings_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    let path = dir.join(format!("{}.md", date.format("%Y-%m-%d")));
    std::fs::write(&path, markdown)
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
    Ok(path)
}

fn project_section(config: &GlobalConfig, project: &Project) -> String {
    let root = PathBuf::from(strip_win_prefix(&project.path));
    let mut md = format!("\n## {}\n", project.name);

    if !root.exists() {
        md.push_str("- ⚠️ path no longer exists (run `void doctor`)\n");
        return md;
    }

    // Services (inventory — live state belongs to `void status`).
    if project.services.is_empty() {
        md.push_str("- services: none\n");
    } else {
        let enabled: Vec<&str> = project
            .services
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.name.as_str())
            .collect();
        md.push_str(&format!(
            "- services: {}/{} enabled ({})\n",
            enabled.len(),
            project.services.len(),
            enabled.join(", ")
        ));
    }

    md.push_str(&debt_section(&root));

    // One audit run feeds both the delta section and the CVE line.
    let audit = crate::audit::audit_project(&project.name, &root);
    md.push_str(&audit_section(project, &audit));
    md.push_str(&deps_cve_section(&audit));
    md.push_str(&contracts_section(config, project));
    md.push_str(&deadcode_section(project));
    md.push_str(&board_section(&root, &project.name));
    md
}

/// Current dependency CVEs (from the audit's npm/pip/cargo/go scans).
fn deps_cve_section(audit: &crate::audit::AuditResult) -> String {
    use crate::audit::{FindingCategory, Severity};
    let cves: Vec<_> = audit
        .findings
        .iter()
        .filter(|f| f.category == FindingCategory::DependencyVulnerability)
        .collect();
    if cves.is_empty() {
        return "- deps: no known CVEs\n".to_string();
    }
    let serious = cves
        .iter()
        .filter(|f| matches!(f.adjusted_severity, Severity::Critical | Severity::High))
        .count();
    let mut md = format!(
        "- deps: {} CVE finding(s), {} critical/high\n",
        cves.len(),
        serious
    );
    for f in cves.iter().take(5) {
        md.push_str(&format!("  - [{}] {}\n", f.adjusted_severity, f.title));
    }
    if cves.len() > 5 {
        md.push_str(&format!("  - (+{} more)\n", cves.len() - 5));
    }
    md
}

/// Cross-project contract drift (Fase 8 check).
#[cfg(feature = "vector")]
fn contracts_section(config: &GlobalConfig, project: &Project) -> String {
    let report = crate::vector_index::contracts_check::check_contracts(config, project);
    if report.consumed == 0 {
        return String::new();
    }
    if report.violations.is_empty() {
        return format!(
            "- contracts: {} consumed, all matched ({} external)\n",
            report.consumed,
            report.external.len()
        );
    }
    let mut md = format!("- contracts: {} DRIFTED\n", report.violations.len());
    for v in report.violations.iter().take(5) {
        md.push_str(&format!(
            "  - {} — {} ({})\n",
            v.contract, v.what_changed, v.producer_project
        ));
    }
    md
}

#[cfg(not(feature = "vector"))]
fn contracts_section(_config: &GlobalConfig, _project: &Project) -> String {
    String::new()
}

/// Debt trend from the two most recent analysis snapshots.
fn debt_section(root: &Path) -> String {
    let snapshots = crate::analyzer::history::load_snapshots(root);
    if snapshots.len() < 2 {
        return "- debt: no history (need ≥2 `void analyze` snapshots)\n".to_string();
    }
    let previous = &snapshots[snapshots.len() - 2];
    let current = &snapshots[snapshots.len() - 1];
    let cmp = crate::analyzer::history::compare(previous, current);
    let (mut better, mut worse) = (0, 0);
    let mut loc_delta = 0i64;
    let mut antipattern_delta = 0i32;
    for svc in &cmp.services {
        loc_delta += svc.loc_delta;
        antipattern_delta += svc.antipattern_delta;
        if svc.antipattern_delta < 0 {
            better += 1;
        } else if svc.antipattern_delta > 0 {
            worse += 1;
        }
    }
    format!(
        "- debt (vs {}): {:?} — LOC {:+}, anti-patterns {:+} ({} improving, {} degrading)\n",
        cmp.previous.format("%Y-%m-%d"),
        cmp.overall_trend,
        loc_delta,
        antipattern_delta,
        better,
        worse
    )
}

/// New audit findings since the previous briefing (delta by finding id).
fn audit_section(project: &Project, result: &crate::audit::AuditResult) -> String {
    let current_ids: HashSet<String> = result.findings.iter().map(|f| f.id.clone()).collect();
    let previous_ids = load_audit_state(&project.name).unwrap_or_default();
    let new: Vec<_> = result
        .findings
        .iter()
        .filter(|f| !previous_ids.contains(&f.id))
        .collect();
    // Persist the full current set for the next delta.
    let _ = save_audit_state(&project.name, &current_ids);

    let mut md = format!(
        "- audit: {} findings (risk {:.0}), {} NEW since last briefing\n",
        result.summary.total,
        result.summary.risk_score,
        new.len()
    );
    for f in new.iter().take(MAX_NEW_FINDINGS) {
        md.push_str(&format!(
            "  - [{}] {} — `{}:{}`\n",
            f.adjusted_severity,
            f.title,
            f.file_path.as_deref().unwrap_or("?"),
            f.line_number.unwrap_or(0)
        ));
    }
    if new.len() > MAX_NEW_FINDINGS {
        md.push_str(&format!("  - (+{} more)\n", new.len() - MAX_NEW_FINDINGS));
    }
    md
}

#[cfg(feature = "structural")]
fn deadcode_section(project: &Project) -> String {
    if !crate::structural::structural_db_path(project).exists() {
        return "- dead code: n/a (no structural graph)\n".to_string();
    }
    match crate::deadcode::find_dead_code(project, 50) {
        Ok(report) => format!(
            "- dead code: {} candidates ({} uncertain)\n",
            report.total_found, report.uncertain_possibly_referenced
        ),
        Err(e) => format!("- dead code: n/a ({})\n", e),
    }
}

#[cfg(not(feature = "structural"))]
fn deadcode_section(_project: &Project) -> String {
    "- dead code: n/a (built without the structural feature)\n".to_string()
}

/// Doing + Review tasks from BOARD.md.
fn board_section(root: &Path, project_name: &str) -> String {
    let Ok(board) = crate::board::load_board(root, project_name) else {
        return String::new();
    };
    let mut md = String::new();
    for col in &board.columns {
        let in_flight =
            col.name.eq_ignore_ascii_case("Doing") || col.name.eq_ignore_ascii_case("Review");
        if !in_flight || col.tasks.is_empty() {
            continue;
        }
        md.push_str(&format!("- board {} ({}):", col.name, col.tasks.len()));
        let titles: Vec<String> = col
            .tasks
            .iter()
            .take(MAX_BOARD_TASKS)
            .map(|t| format!(" **{}** {}", t.id, t.title))
            .collect();
        md.push_str(&titles.join(" ·"));
        if col.tasks.len() > MAX_BOARD_TASKS {
            md.push_str(&format!(" (+{} more)", col.tasks.len() - MAX_BOARD_TASKS));
        }
        md.push('\n');
    }
    md
}

// ── Audit delta state ───────────────────────────────────────

fn audit_state_path(project_name: &str) -> Result<PathBuf, String> {
    Ok(briefings_dir()?
        .join("state")
        .join(format!("{}.json", project_name)))
}

fn load_audit_state(project_name: &str) -> Result<HashSet<String>, String> {
    let path = audit_state_path(project_name)?;
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    serde_json::from_str(&content).map_err(|e| format!("bad state file {}: {}", path.display(), e))
}

fn save_audit_state(project_name: &str, ids: &HashSet<String>) -> Result<(), String> {
    let path = audit_state_path(project_name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string(ids).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("cannot write {}: {}", path.display(), e))
}

/// True when the daemon scheduler should fire: `schedule` matches the
/// current local HH:MM and the briefing hasn't run today. Pure so it's
/// unit-testable.
pub fn schedule_due(
    schedule: Option<&str>,
    now_hhmm: &str,
    today: chrono::NaiveDate,
    last_run: Option<chrono::NaiveDate>,
) -> bool {
    match schedule {
        Some(at) => at == now_hhmm && last_run != Some(today),
        None => false,
    }
}

/// Marker file remembering the last scheduled run date.
pub fn last_run_path() -> Result<PathBuf, String> {
    Ok(briefings_dir()?.join(".last-run"))
}

pub fn read_last_run() -> Option<chrono::NaiveDate> {
    let path = last_run_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    chrono::NaiveDate::parse_from_str(content.trim(), "%Y-%m-%d").ok()
}

pub fn write_last_run(date: chrono::NaiveDate) -> Result<(), String> {
    let dir = briefings_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(last_run_path()?, date.format("%Y-%m-%d").to_string()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global_config::BriefingConfig;
    use crate::model::Project;

    fn project(name: &str, path: &Path) -> Project {
        Project {
            name: name.to_string(),
            path: path.to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    #[test]
    fn test_briefing_requires_active_projects() {
        let config = GlobalConfig::default();
        let err = generate_briefing(&config, None).unwrap_err();
        assert!(err.contains("no active projects"));
    }

    #[test]
    fn test_briefing_covers_active_and_board() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("alpha");
        let b = tmp.path().join("beta");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(
            a.join("BOARD.md"),
            "## Doing\n\n- **VB-2** Ship the briefing\n\n## Review\n\n- **VB-1** Review me\n",
        )
        .unwrap();

        let config = GlobalConfig {
            projects: vec![project("alpha", &a), project("beta", &b)],
            briefing: BriefingConfig {
                active_projects: vec!["alpha".into()],
                ..Default::default()
            },
        };
        let md = generate_briefing(&config, None).unwrap();
        assert!(md.contains("## alpha"));
        assert!(!md.contains("## beta"), "inactive projects stay out:\n{md}");
        assert!(md.contains("board Doing (1)"));
        assert!(md.contains("VB-2"));
        assert!(md.contains("board Review (1)"));
        assert!(md.contains("- debt: no history"));

        // `only` overrides the active list.
        let md = generate_briefing(&config, Some(&["beta".to_string()])).unwrap();
        assert!(md.contains("## beta"));
        assert!(!md.contains("## alpha"));
    }

    #[test]
    fn test_schedule_due() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 9).unwrap();
        let yesterday = today.pred_opt().unwrap();
        assert!(schedule_due(Some("08:30"), "08:30", today, None));
        assert!(schedule_due(Some("08:30"), "08:30", today, Some(yesterday)));
        assert!(!schedule_due(Some("08:30"), "08:30", today, Some(today)));
        assert!(!schedule_due(Some("08:30"), "08:31", today, None));
        assert!(!schedule_due(None, "08:30", today, None));
    }
}
