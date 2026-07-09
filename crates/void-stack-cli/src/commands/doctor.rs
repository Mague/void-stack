//! `void doctor` — sanity checks for the global project registry.

use std::io::Write;

use anyhow::Result;
use void_stack_core::doctor::{self, DoctorFix, IssueKind};
use void_stack_core::global_config::{load_global_config, save_global_config};

fn kind_label(kind: IssueKind) -> &'static str {
    match kind {
        IssueKind::DuplicatePath => "duplicate",
        IssueKind::NestedProject => "nested",
        IssueKind::MissingPath => "missing path",
        IssueKind::BrokenWorkingDir => "broken working_dir",
        IssueKind::OrphanIndex => "orphan index",
        IssueKind::StaleIndex => "stale index",
        IssueKind::StaleGraph => "stale graph",
    }
}

fn confirm(prompt: &str) -> bool {
    print!("{} [y/N] ", prompt);
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

pub fn cmd_doctor(fix: bool, json: bool) -> Result<()> {
    let mut config = load_global_config()?;
    let report = doctor::run_doctor(&config, &doctor::indexes_root());

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!(
        "Doctor — {} project(s) checked, {} issue(s) found\n",
        report.checked_projects,
        report.issues.len()
    );
    if report.issues.is_empty() {
        println!("✓ registry is healthy");
        return Ok(());
    }

    let mut config_dirty = false;
    for issue in &report.issues {
        let who = issue.project.as_deref().unwrap_or("-");
        println!(
            "⚠️  [{}] {} — {}",
            kind_label(issue.kind),
            who,
            issue.detail
        );

        let Some(fix_action) = &issue.fix else {
            println!("    (no automatic fix — resolve manually)");
            continue;
        };

        // Stale artifacts get a command hint, never an auto-run.
        if matches!(
            fix_action,
            DoctorFix::Reindex { .. } | DoctorFix::RebuildGraph { .. }
        ) {
            if let Ok(hint) = doctor::apply_fix(&mut config, fix_action) {
                println!("    → {}", hint);
            }
            continue;
        }

        if !fix {
            println!("    → re-run with --fix to resolve");
            continue;
        }
        if confirm(&format!("    Apply fix ({:?})?", fix_action)) {
            match doctor::apply_fix(&mut config, fix_action) {
                Ok(msg) => {
                    println!("    ✓ {}", msg);
                    if !matches!(fix_action, DoctorFix::DeleteIndexDir { .. }) {
                        config_dirty = true;
                    }
                }
                Err(e) => println!("    ❌ {}", e),
            }
        }
    }

    if config_dirty {
        save_global_config(&config)?;
        println!("\n✓ registry saved");
    }
    Ok(())
}
