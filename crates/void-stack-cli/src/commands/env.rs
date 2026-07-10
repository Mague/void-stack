//! `void env check` — env vars the code reads vs .env.example.

use std::path::PathBuf;

use anyhow::Result;
use void_stack_core::envcheck;
use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::runner::local::strip_win_prefix;

pub fn cmd_env_check(project_name: &str, write: bool) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();
    let root = PathBuf::from(strip_win_prefix(&project.path));

    let report = envcheck::check_env(&root);
    println!(
        "Env — {}: {} var(s) read by the code, example file: {}\n",
        project.name,
        report.used,
        report.example_file.as_deref().unwrap_or("none")
    );
    if !report.undocumented.is_empty() {
        println!("Used but NOT documented ({}):", report.undocumented.len());
        for u in &report.undocumented {
            println!("  ⚠️  {} (first read at {})", u.name, u.site);
        }
    }
    if !report.dead.is_empty() {
        println!("Documented but never read ({}):", report.dead.len());
        for d in &report.dead {
            println!("  💀 {}", d);
        }
    }
    if report.undocumented.is_empty() && report.dead.is_empty() {
        println!(
            "✓ code and {} agree",
            report.example_file.as_deref().unwrap_or(".env.example")
        );
    }

    if write {
        let path = envcheck::write_env_example(&root, &report).map_err(|e| anyhow::anyhow!(e))?;
        println!(
            "\n✓ {} updated ({} name(s) appended, comments preserved, no real values)",
            path.display(),
            report.undocumented.len()
        );
    } else if !report.undocumented.is_empty() {
        println!("\nrun with --write to append the missing names to the example file");
    }
    Ok(())
}
