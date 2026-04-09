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
        println!("  No se encontraron problemas de seguridad.\n");
    } else {
        println!("  Hallazgos:");
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

        // Print findings
        for finding in &result.findings {
            let icon = match finding.severity {
                audit::Severity::Critical => "[CRIT]",
                audit::Severity::High => "[HIGH]",
                audit::Severity::Medium => "[MED ]",
                audit::Severity::Low => "[LOW ]",
                audit::Severity::Info => "[INFO]",
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
