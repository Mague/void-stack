use std::path::Path;

use anyhow::Result;

use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::runner::local::strip_win_prefix;

pub async fn cmd_audit(project_name: &str, output: Option<&str>) -> Result<()> {
    use void_stack_core::audit;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let clean_path = strip_win_prefix(&project.path);
    println!("Running security audit for '{}'...\n", project.name);

    let project_name_owned = project.name.clone();
    let clean_path_owned = clean_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        audit::audit_project(&project_name_owned, Path::new(&clean_path_owned))
    })
    .await
    .map_err(|e| anyhow::anyhow!("Audit task panicked: {}", e))?;

    // Print summary
    if result.summary.total == 0 {
        println!("  No security issues found.\n");
    } else {
        println!("  Findings:");
        if result.summary.critical > 0 {
            println!("    Critical: {}", result.summary.critical);
        }
        if result.summary.high > 0 {
            println!("    High:     {}", result.summary.high);
        }
        if result.summary.medium > 0 {
            println!("    Medium:   {}", result.summary.medium);
        }
        if result.summary.low > 0 {
            println!("    Low:      {}", result.summary.low);
        }
        if result.summary.info > 0 {
            println!("    Info:     {}", result.summary.info);
        }
        println!("    Total:       {}", result.summary.total);
        println!("    Risk Score:  {:.0}/100\n", result.summary.risk_score);
        if result.suppressed > 0 {
            println!("    Suppressed:  {}\n", result.suppressed);
        }

        // Print findings — use adjusted_severity for display
        for finding in &result.findings {
            let effective = finding.adjusted_severity;
            let icon = match effective {
                audit::Severity::Critical => "[CRIT]",
                audit::Severity::High => "[HIGH]",
                audit::Severity::Medium => "[MED ]",
                audit::Severity::Low => "[LOW ]",
                audit::Severity::Info => "[INFO]",
            };
            // Skip Info findings in CLI output to reduce noise
            if effective == audit::Severity::Info {
                continue;
            }
            println!("  {} [{}] {}", icon, effective, finding.title);
            println!("     {}", finding.description);
            if let Some(ref path) = finding.file_path {
                if let Some(line) = finding.line_number {
                    println!("     File: {}:{}", path, line);
                } else {
                    println!("     File: {}", path);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testutil::{config_lock, isolate_data_dir, register_project, unique_name};

    /// Drive an async command on a current-thread runtime so the
    /// `config_lock` guard never spans an await point.
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(fut)
    }

    #[test]
    fn test_cmd_audit_not_found() {
        let _guard = config_lock();
        isolate_data_dir();
        let err = block_on(cmd_audit("no-such-project-xyz", None)).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn test_cmd_audit_writes_report_for_clean_project() {
        let _guard = config_lock();
        isolate_data_dir();
        let tmp = tempfile::tempdir().unwrap();
        // A single innocuous source file — no secrets, so the summary is empty.
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let name = unique_name("audit");
        register_project(&name, tmp.path());

        let out = tmp.path().join("report.md");
        block_on(cmd_audit(&name, Some(&out.to_string_lossy()))).unwrap();

        assert!(out.is_file(), "audit report should be written");
        let report = std::fs::read_to_string(&out).unwrap();
        assert!(!report.is_empty());
    }

    #[test]
    fn test_cmd_audit_prints_and_reports_findings() {
        let _guard = config_lock();
        isolate_data_dir();
        let tmp = tempfile::tempdir().unwrap();
        // A hardcoded AWS access key triggers a secret finding, exercising
        // the non-empty-summary printing branches (icons, file path, fix).
        std::fs::write(
            tmp.path().join("config.py"),
            "AWS_KEY = \"AKIAIOSFODNN7ABCDEFGH\"\n",
        )
        .unwrap();
        let name = unique_name("audit-findings");
        register_project(&name, tmp.path());

        let out = tmp.path().join("report.md");
        block_on(cmd_audit(&name, Some(&out.to_string_lossy()))).unwrap();

        let report = std::fs::read_to_string(&out).unwrap();
        assert!(!report.is_empty(), "report with findings should be written");
    }
}
