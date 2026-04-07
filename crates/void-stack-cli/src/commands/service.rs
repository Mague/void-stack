use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use void_stack_core::backend::ServiceBackend;
use void_stack_core::global_config::{find_project, load_global_config};
use void_stack_core::manager::ProcessManager;
use void_stack_core::model::*;
use void_stack_proto::client::DaemonClient;

// ── Start ────────────────────────────────────────────────────

pub async fn cmd_start(
    daemon: bool,
    port: u16,
    project_name: &str,
    service: Option<&str>,
) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Project '{}' not found. Use 'void list' to see available projects.",
                project_name
            )
        })?
        .clone();

    // Pre-check dependencies before starting
    {
        use void_stack_core::detector::{self, CheckStatus};
        let stripped = void_stack_core::runner::local::strip_win_prefix(&project.path);
        let mut dirs: Vec<std::path::PathBuf> = vec![Path::new(&stripped).to_path_buf()];
        for svc in &project.services {
            if let Some(dir) = &svc.working_dir {
                let s = void_stack_core::runner::local::strip_win_prefix(dir);
                let p = Path::new(&s).to_path_buf();
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        let mut warnings = Vec::new();
        for dir in &dirs {
            for dep in detector::check_project(dir).await {
                if seen.insert(format!("{:?}", dep.dep_type)) {
                    match dep.status {
                        CheckStatus::Ok => {}
                        _ => {
                            let hint = dep.fix_hint.as_deref().unwrap_or("");
                            warnings.push(format!(
                                "  {} {} — {}{}",
                                match dep.status {
                                    CheckStatus::Missing => "❌",
                                    CheckStatus::NotRunning => "⚠️",
                                    CheckStatus::NeedsSetup => "🔧",
                                    _ => "❓",
                                },
                                dep.dep_type,
                                dep.details.first().map(|s| s.as_str()).unwrap_or(""),
                                if hint.is_empty() {
                                    String::new()
                                } else {
                                    format!(" (fix: {})", hint)
                                },
                            ));
                        }
                    }
                }
            }
        }
        if !warnings.is_empty() {
            println!("Dependency warnings:");
            for w in &warnings {
                println!("{}", w);
            }
            println!();
        }
    }

    let backend: Box<dyn ServiceBackend> = if daemon {
        let addr = format!("http://127.0.0.1:{}", port);
        let client = DaemonClient::connect_with_timeout(&addr, Duration::from_secs(5))
            .await
            .context("Cannot connect to daemon.")?;
        Box::new(client)
    } else {
        Box::new(ProcessManager::new(project.clone()))
    };

    let running_count = match service {
        Some(name) => {
            let state = backend.start_one(name).await?;
            println!(
                "  {} {} (pid: {:?})",
                status_icon(&state.status),
                state.service_name,
                state.pid
            );
            1usize
        }
        None => {
            let states = backend.start_all().await?;
            println!("Project: {}\n", project_name);
            for state in &states {
                println!(
                    "  {} {} (pid: {:?})",
                    status_icon(&state.status),
                    state.service_name,
                    state.pid,
                );
            }
            states
                .iter()
                .filter(|s| s.status == ServiceStatus::Running)
                .count()
        }
    };

    if running_count == 0 {
        println!("\n  No services started successfully.");
        return Ok(());
    }

    println!(
        "\n  {} services running. Detecting URLs... (Ctrl+C to stop all)",
        running_count,
    );

    // Continuously poll for URLs while waiting for Ctrl+C
    let mut urls_found: HashMap<String, String> = HashMap::new();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                let updated_states = backend.get_states().await?;
                for state in &updated_states {
                    if let Some(url) = &state.url
                        && !urls_found.contains_key(&state.service_name) {
                            urls_found.insert(state.service_name.clone(), url.clone());
                            println!("    {} → {}", state.service_name, url);
                        }
                }
            }
        }
    }

    println!("\nStopping all services...");
    backend.stop_all().await?;
    println!("Done.");

    Ok(())
}

// ── Stop ─────────────────────────────────────────────────────

pub async fn cmd_stop(
    daemon: bool,
    port: u16,
    project_name: &str,
    service: Option<&str>,
) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();

    let backend: Box<dyn ServiceBackend> = if daemon {
        let addr = format!("http://127.0.0.1:{}", port);
        let client = DaemonClient::connect_with_timeout(&addr, Duration::from_secs(5))
            .await
            .context("Cannot connect to daemon.")?;
        Box::new(client)
    } else {
        Box::new(ProcessManager::new(project))
    };

    match service {
        Some(name) => {
            backend.stop_one(name).await?;
            println!("Stopped: {}", name);
        }
        None => {
            backend.stop_all().await?;
            println!("All services of '{}' stopped.", project_name);
        }
    }
    Ok(())
}

// ── Status ───────────────────────────────────────────────────

pub async fn cmd_status(project_name: Option<&str>) -> Result<()> {
    let config = load_global_config()?;

    match project_name {
        None => {
            // Overview of all projects
            if config.projects.is_empty() {
                println!("No projects registered. Use 'void add <name> <path>'.");
                return Ok(());
            }
            for project in &config.projects {
                println!("  {} ({} services)", project.name, project.services.len());
            }
        }
        Some(name) => {
            let project = find_project(&config, name)
                .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", name))?;
            println!("Project: {}", project.name);
            println!("Path:    {}", project.path);
            println!("\nServices:");
            for svc in &project.services {
                let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
                println!(
                    "  {} [{}] {}",
                    if svc.enabled { "●" } else { "○" },
                    svc.target,
                    svc.name,
                );
                println!("    cmd: {}", svc.command);
                println!("    dir: {}", dir);
            }
        }
    }
    Ok(())
}

// ── Logs ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn cmd_logs(
    daemon: bool,
    port: u16,
    project_name: &str,
    service_name: &str,
    lines: usize,
    compact: bool,
    raw: bool,
) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();

    let backend: Box<dyn ServiceBackend> = if daemon {
        let addr = format!("http://127.0.0.1:{}", port);
        let client = DaemonClient::connect_with_timeout(&addr, Duration::from_secs(5))
            .await
            .context("Cannot connect to daemon.")?;
        Box::new(client)
    } else {
        Box::new(ProcessManager::new(project))
    };

    let all_logs = backend.get_logs(service_name).await?;
    let n = lines.clamp(1, 5000);
    let start = all_logs.len().saturating_sub(n);
    let recent = &all_logs[start..];

    if recent.is_empty() {
        println!("No logs captured for service '{}'.", service_name);
        return Ok(());
    }

    if raw {
        for line in recent {
            println!("{}", line);
        }
    } else {
        let joined = recent.join("\n");
        let result =
            void_stack_core::log_filter::filter_log_output_tracked(&joined, compact, project_name);
        println!("{}", result.content);

        if result.savings_pct > 20.0 {
            println!(
                "\n[Filtrado: {}→{} líneas, ahorro {:.0}%]",
                result.lines_original, result.lines_filtered, result.savings_pct
            );
        }
    }

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────

fn status_icon(status: &ServiceStatus) -> &'static str {
    match status {
        ServiceStatus::Running => "●",
        ServiceStatus::Stopped => "○",
        ServiceStatus::Starting => "◐",
        ServiceStatus::Failed => "✗",
        ServiceStatus::Stopping => "◑",
    }
}
