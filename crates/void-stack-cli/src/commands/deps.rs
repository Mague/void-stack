use std::path::Path;

use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};

pub async fn cmd_check(project_name: &str) -> Result<()> {
    use void_stack_core::detector::{self, CheckStatus};

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    println!("Checking dependencies for '{}'...\n", project.name);

    // Collect all unique directories to scan
    let mut dirs_to_check: Vec<std::path::PathBuf> = vec![];
    let stripped = void_stack_core::runner::local::strip_win_prefix(&project.path);
    dirs_to_check.push(Path::new(&stripped).to_path_buf());

    for svc in &project.services {
        if let Some(dir) = &svc.working_dir {
            let stripped = void_stack_core::runner::local::strip_win_prefix(dir);
            let p = Path::new(&stripped).to_path_buf();
            if !dirs_to_check.contains(&p) {
                dirs_to_check.push(p);
            }
        }
    }

    // Run checks on each directory, dedup results by dep_type
    let mut seen = std::collections::HashSet::new();
    let mut all_results = Vec::new();

    for dir in &dirs_to_check {
        let results = detector::check_project(dir).await;
        for result in results {
            if seen.insert(format!("{:?}", result.dep_type)) {
                all_results.push(result);
            }
        }
    }

    if all_results.is_empty() {
        println!("  No dependencies detected for this project.");
        return Ok(());
    }

    for dep in &all_results {
        let icon = match dep.status {
            CheckStatus::Ok => "✅",
            CheckStatus::Missing => "❌",
            CheckStatus::NotRunning => "⚠️",
            CheckStatus::NeedsSetup => "🔧",
            CheckStatus::Unknown => "❓",
        };

        let ver = dep.version.as_deref().unwrap_or("");
        println!("  {} {} {}", icon, dep.dep_type, ver);

        for detail in &dep.details {
            println!("     {}", detail);
        }

        if let Some(hint) = &dep.fix_hint {
            println!("     fix: {}", hint);
        }
        println!();
    }

    let ok_count = all_results.iter().filter(|d| matches!(d.status, CheckStatus::Ok)).count();
    let total = all_results.len();
    println!("  {}/{} dependencies ready.", ok_count, total);

    Ok(())
}
