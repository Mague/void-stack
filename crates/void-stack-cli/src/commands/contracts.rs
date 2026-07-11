//! `void contracts check` — fail on cross-project contract drift.

use anyhow::Result;
use void_stack_core::global_config::{find_project, load_global_config};

pub fn cmd_contracts_check(project_name: &str) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();

    let report = void_stack_core::vector_index::contracts_check::check_contracts(&config, &project);

    println!(
        "Contracts — {}: {} consumed, {} matched, {} external, {} violation(s)\n",
        report.project,
        report.consumed,
        report.matched,
        report.external.len(),
        report.violations.len()
    );
    for v in &report.violations {
        println!("❌ {} (consumed at {})", v.contract, v.consumer_site);
        println!("   producer: {} — {}", v.producer_project, v.what_changed);
    }
    if !report.external.is_empty() {
        println!(
            "ℹ external (no registered producer): {}",
            report.external.join(", ")
        );
    }
    if report.violations.is_empty() {
        println!("✓ no contract drift");
        Ok(())
    } else {
        // Non-zero exit so this works as a pre-commit / CI gate.
        anyhow::bail!("{} contract violation(s)", report.violations.len())
    }
}
