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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil;

    #[test]
    fn test_env_check_unknown_project_errors() {
        let _guard = testutil::config_lock();
        testutil::isolate_data_dir();
        let err = cmd_env_check("cli-no-such-project-xyz", false).unwrap_err();
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    #[test]
    fn test_env_check_reports_without_writing() {
        let _guard = testutil::config_lock();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("main.py"),
            "import os\nkey = os.getenv(\"API_KEY\")\n",
        )
        .unwrap();
        std::fs::write(root.join(".env.example"), "# example\nDEAD_VAR=\n").unwrap();
        let name = testutil::unique_name("env-check");
        testutil::register_project(&name, root);

        cmd_env_check(&name, false).unwrap();

        // Without --write the example file stays untouched.
        let example = std::fs::read_to_string(root.join(".env.example")).unwrap();
        assert!(!example.contains("API_KEY"));
        assert!(example.contains("DEAD_VAR"));
    }

    #[test]
    fn test_env_check_write_appends_missing_names() {
        let _guard = testutil::config_lock();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("main.py"),
            "import os\nkey = os.getenv(\"API_KEY\")\nurl = os.environ[\"BASE_URL\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".env.example"),
            "# keep this comment\nDEAD_VAR=\n",
        )
        .unwrap();
        let name = testutil::unique_name("env-write");
        testutil::register_project(&name, root);

        cmd_env_check(&name, true).unwrap();

        let example = std::fs::read_to_string(root.join(".env.example")).unwrap();
        assert!(
            example.contains("API_KEY"),
            "missing name appended:\n{example}"
        );
        assert!(example.contains("BASE_URL"));
        // Existing comments and entries are preserved, no real values leak.
        assert!(example.contains("# keep this comment"));
        assert!(example.contains("DEAD_VAR"));
    }
}
