use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use devlaunch_core::backend::ServiceBackend;
use devlaunch_core::config;
use devlaunch_core::global_config::{
    self, default_command_for, default_command_for_dir, find_project, load_global_config, remove_project,
    save_global_config, scan_subprojects,
};
use devlaunch_core::manager::ProcessManager;
use devlaunch_core::model::*;
use devlaunch_proto::client::DaemonClient;

const DEFAULT_DAEMON_PORT: u16 = 50051;

#[derive(Parser)]
#[command(name = "devlaunch", about = "Unified dev service launcher & monitor")]
struct Cli {
    /// Connect to daemon instead of managing processes directly
    #[arg(long)]
    daemon: bool,

    /// Daemon port (used with --daemon)
    #[arg(long, default_value_t = DEFAULT_DAEMON_PORT)]
    port: u16,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a project (scan directory for services)
    Add {
        /// Project name
        name: String,
        /// Root path of the project
        path: String,
        /// Project is inside WSL (path is a Linux path like /home/user/project)
        #[arg(long)]
        wsl: bool,
    },

    /// Add a service to an existing project
    #[command(name = "add-service")]
    AddService {
        /// Project name
        project: String,
        /// Service name
        name: String,
        /// Command to run
        command: String,
        /// Working directory (absolute path)
        #[arg(short = 'd', long)]
        dir: String,
        /// Target: windows, wsl, docker, ssh
        #[arg(short, long, default_value = "windows")]
        target: String,
    },

    /// Remove a project
    Remove {
        /// Project name to remove
        name: String,
    },

    /// List all registered projects and their services
    List,

    /// Start all services of a project (or a specific one)
    Start {
        /// Project name
        project: String,
        /// Specific service to start (omit for all)
        #[arg(short, long)]
        service: Option<String>,
    },

    /// Stop all services of a project (or a specific one)
    Stop {
        /// Project name
        project: String,
        /// Specific service to stop (omit for all)
        #[arg(short, long)]
        service: Option<String>,
    },

    /// Show live status of a project's services
    Status {
        /// Project name (omit for all projects overview)
        project: Option<String>,
    },

    /// Check dependencies for a project (Python, Node, CUDA, Ollama, Docker, .env)
    Check {
        /// Project name
        project: String,
    },

    /// Scan a directory and show what devlaunch detects
    Scan {
        /// Path to scan
        #[arg(default_value = ".")]
        path: String,
        /// Scan inside WSL (path is a Linux path)
        #[arg(long)]
        wsl: bool,
    },

    /// Initialize a devlaunch.toml in a directory (legacy/local mode)
    Init {
        /// Path to project directory
        #[arg(default_value = ".")]
        path: String,
    },

    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon for a project
    Start {
        /// Project name
        project: String,
        /// gRPC listen port
        #[arg(long, default_value_t = DEFAULT_DAEMON_PORT)]
        port: u16,
    },
    /// Stop the running daemon
    Stop,
    /// Check daemon status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::Add { name, path, wsl } => cmd_add(name, path, *wsl)?,
        Commands::AddService { project, name, command, dir, target } => {
            cmd_add_service(project, name, command, dir, target)?;
        }
        Commands::Remove { name } => cmd_remove(name)?,
        Commands::List => cmd_list()?,
        Commands::Check { project } => cmd_check(project).await?,
        Commands::Scan { path, wsl } => cmd_scan(path, *wsl),
        Commands::Init { path } => cmd_init(path)?,
        Commands::Start { project, service } => {
            cmd_start(&cli, project, service.as_deref()).await?;
        }
        Commands::Stop { project, service } => {
            cmd_stop(&cli, project, service.as_deref()).await?;
        }
        Commands::Status { project } => cmd_status(project.as_deref()).await?,
        Commands::Daemon { action } => match action {
            DaemonAction::Start { project, port } => cmd_daemon_start(project, *port).await?,
            DaemonAction::Stop => cmd_daemon_stop().await?,
            DaemonAction::Status => cmd_daemon_status().await?,
        },
    }

    Ok(())
}

// ── Add project ──────────────────────────────────────────────

fn cmd_add(name: &str, path: &str, wsl: bool) -> Result<()> {
    let mut config = load_global_config()?;

    if find_project(&config, name).is_some() {
        bail!("Project '{}' already exists. Use 'devlaunch remove {}' first.", name, name);
    }

    let default_target = if wsl { Target::Wsl } else { Target::Windows };

    let services: Vec<Service> = if wsl {
        // WSL project: scan via wsl commands
        let detected = global_config::scan_wsl_subprojects(path);
        if detected.is_empty() {
            println!("No services auto-detected in WSL path. Add them manually with 'devlaunch add-service'.");
            vec![]
        } else {
            println!("Detected {} service(s) in WSL {}:", detected.len(), path);
            detected
                .iter()
                .enumerate()
                .map(|(i, (sub_name, sub_path, pt))| {
                    let svc_name = sub_name.replace('/', "-");
                    let cmd = default_command_for(*pt);
                    println!("  {}. {} ({:?}) → {}", i + 1, svc_name, pt, sub_path);
                    Service {
                        name: svc_name,
                        command: cmd,
                        target: Target::Wsl,
                        working_dir: Some(sub_path.clone()),
                        enabled: true,
                        env_vars: vec![],
                        depends_on: vec![],
                    }
                })
                .collect()
        }
    } else {
        // Windows project: scan local filesystem
        let abs_path = std::fs::canonicalize(path)
            .with_context(|| format!("Path not found: {}", path))?;
        let detected = scan_subprojects(&abs_path);
        if detected.is_empty() {
            println!("No services auto-detected. Add them manually with 'devlaunch add-service'.");
            vec![]
        } else {
            println!("Detected {} service(s) in {}:", detected.len(), abs_path.display());
            detected
                .iter()
                .enumerate()
                .map(|(i, (sub_name, sub_path, pt))| {
                    let svc_name = sub_name.replace('/', "-").replace('\\', "-");
                    let cmd = default_command_for_dir(*pt, sub_path);
                    println!(
                        "  {}. {} ({:?}) → {}",
                        i + 1, svc_name, pt, sub_path.display()
                    );
                    Service {
                        name: svc_name,
                        command: cmd,
                        target: default_target,
                        working_dir: Some(sub_path.to_string_lossy().to_string()),
                        enabled: true,
                        env_vars: vec![],
                        depends_on: vec![],
                    }
                })
                .collect()
        }
    };

    let project_path = if wsl {
        path.to_string()
    } else {
        std::fs::canonicalize(path)
            .with_context(|| format!("Path not found: {}", path))?
            .to_string_lossy()
            .to_string()
    };

    let project = Project {
        name: name.to_string(),
        description: String::new(),
        path: project_path,
        project_type: None,
        tags: vec![],
        services,
        hooks: None,
    };

    config.projects.push(project);
    save_global_config(&config)?;
    println!("\nProject '{}' added. Edit services with 'devlaunch add-service' or edit the config directly.", name);
    println!("Config: {}", global_config::global_config_path()?.display());

    Ok(())
}

// ── Add service to project ───────────────────────────────────

fn cmd_add_service(project_name: &str, svc_name: &str, command: &str, dir: &str, target: &str) -> Result<()> {
    let mut config = load_global_config()?;

    let project = config
        .projects
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(project_name))
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found. Use 'devlaunch add' first.", project_name))?;

    if project.services.iter().any(|s| s.name == svc_name) {
        bail!("Service '{}' already exists in project '{}'.", svc_name, project_name);
    }

    let target_enum = match target.to_lowercase().as_str() {
        "windows" => Target::Windows,
        "wsl" => Target::Wsl,
        "docker" => Target::Docker,
        "ssh" => Target::Ssh,
        _ => bail!("Invalid target '{}'. Use: windows, wsl, docker, ssh", target),
    };

    let abs_dir = std::fs::canonicalize(dir)
        .with_context(|| format!("Directory not found: {}", dir))?
        .to_string_lossy()
        .to_string();

    project.services.push(Service {
        name: svc_name.to_string(),
        command: command.to_string(),
        target: target_enum,
        working_dir: Some(abs_dir.clone()),
        enabled: true,
        env_vars: vec![],
        depends_on: vec![],
    });

    save_global_config(&config)?;
    println!("Service '{}' added to '{}' (dir: {})", svc_name, project_name, abs_dir);

    Ok(())
}

// ── Remove project ───────────────────────────────────────────

fn cmd_remove(name: &str) -> Result<()> {
    let mut config = load_global_config()?;
    if remove_project(&mut config, name) {
        save_global_config(&config)?;
        println!("Project '{}' removed.", name);
    } else {
        println!("Project '{}' not found.", name);
    }
    Ok(())
}

// ── List projects ────────────────────────────────────────────

fn cmd_list() -> Result<()> {
    let config = load_global_config()?;

    if config.projects.is_empty() {
        println!("No projects registered.");
        println!("Add one with: devlaunch add <name> <path>");
        return Ok(());
    }

    for project in &config.projects {
        println!("{}", project.name);
        println!("  path: {}", project.path);
        if project.services.is_empty() {
            println!("  (no services)");
        } else {
            for svc in &project.services {
                let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
                println!(
                    "  {} [{}] {} → {}",
                    if svc.enabled { "●" } else { "○" },
                    svc.target,
                    svc.name,
                    dir,
                );
                println!("    cmd: {}", svc.command);
            }
        }
        println!();
    }

    Ok(())
}

// ── Scan directory ───────────────────────────────────────────

fn cmd_scan(path: &str, wsl: bool) {
    if wsl {
        println!("Scanning WSL {}...\n", path);
        let detected = global_config::scan_wsl_subprojects(path);
        if detected.is_empty() {
            println!("No projects detected.");
        } else {
            for (name, sub_path, pt) in &detected {
                println!("  {:?} → {} ({})", pt, name, sub_path);
            }
            println!(
                "\nUse 'devlaunch add <name> {} --wsl' to register this project.",
                path
            );
        }
    } else {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
        println!("Scanning {}...\n", abs.display());
        let detected = scan_subprojects(&abs);
        if detected.is_empty() {
            println!("No projects detected.");
        } else {
            for (name, sub_path, pt) in &detected {
                println!("  {:?} → {} ({})", pt, name, sub_path.display());
            }
            println!(
                "\nUse 'devlaunch add <name> {}' to register this project.",
                abs.display()
            );
        }
    }
}

// ── Init (legacy local mode) ─────────────────────────────────

fn cmd_init(path: &str) -> Result<()> {
    let dir = Path::new(path);
    let project_type = config::detect_project_type(dir);

    let project = Project {
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "my-project".to_string()),
        description: String::new(),
        path: path.to_string(),
        project_type: Some(project_type),
        tags: vec![],
        services: vec![Service {
            name: "main".to_string(),
            command: default_command_for_dir(project_type, std::path::Path::new(path)),
            target: Target::Windows,
            working_dir: None,
            enabled: true,
            env_vars: vec![],
            depends_on: vec![],
        }],
        hooks: Some(HookConfig {
            venv: project_type == ProjectType::Python,
            install_deps: true,
            build: false,
            custom: vec![],
        }),
    };

    config::save_project(&project, dir)?;
    println!("Created devlaunch.toml ({:?} project detected)", project_type);
    Ok(())
}

// ── Check dependencies ───────────────────────────────────────

async fn cmd_check(project_name: &str) -> Result<()> {
    use devlaunch_core::detector::{self, CheckStatus};

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    println!("Checking dependencies for '{}'...\n", project.name);

    // Collect all unique directories to scan
    let mut dirs_to_check: Vec<std::path::PathBuf> = vec![];
    let stripped = devlaunch_core::runner::local::strip_win_prefix(&project.path);
    dirs_to_check.push(Path::new(&stripped).to_path_buf());

    for svc in &project.services {
        if let Some(dir) = &svc.working_dir {
            let stripped = devlaunch_core::runner::local::strip_win_prefix(dir);
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

// ── Start ────────────────────────────────────────────────────

async fn cmd_start(cli: &Cli, project_name: &str, service: Option<&str>) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found. Use 'devlaunch list' to see available projects.", project_name))?
        .clone();

    let backend: Box<dyn ServiceBackend> = if cli.daemon {
        let addr = format!("http://127.0.0.1:{}", cli.port);
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
            states.iter().filter(|s| s.status == ServiceStatus::Running).count()
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
                    if let Some(url) = &state.url {
                        if !urls_found.contains_key(&state.service_name) {
                            urls_found.insert(state.service_name.clone(), url.clone());
                            println!("    {} → {}", state.service_name, url);
                        }
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

async fn cmd_stop(cli: &Cli, project_name: &str, service: Option<&str>) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?
        .clone();

    let backend: Box<dyn ServiceBackend> = if cli.daemon {
        let addr = format!("http://127.0.0.1:{}", cli.port);
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

async fn cmd_status(project_name: Option<&str>) -> Result<()> {
    let config = load_global_config()?;

    match project_name {
        None => {
            // Overview of all projects
            if config.projects.is_empty() {
                println!("No projects registered. Use 'devlaunch add <name> <path>'.");
                return Ok(());
            }
            for project in &config.projects {
                println!(
                    "  {} ({} services)",
                    project.name,
                    project.services.len()
                );
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

// ── Daemon commands ──────────────────────────────────────────

async fn cmd_daemon_start(project_name: &str, port: u16) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    println!("To start the daemon, run:");
    println!(
        "  devlaunch-daemon start -p \"{}\" --port {}",
        project.path, port
    );
    Ok(())
}

async fn cmd_daemon_stop() -> Result<()> {
    let addr = format!("http://127.0.0.1:{}", DEFAULT_DAEMON_PORT);
    match DaemonClient::connect_with_timeout(&addr, Duration::from_secs(3)).await {
        Ok(mut client) => {
            client.shutdown().await?;
            println!("Daemon shutdown initiated.");
        }
        Err(_) => {
            println!(
                "No daemon is running (cannot connect on port {}).",
                DEFAULT_DAEMON_PORT
            );
        }
    }
    Ok(())
}

async fn cmd_daemon_status() -> Result<()> {
    let addr = format!("http://127.0.0.1:{}", DEFAULT_DAEMON_PORT);
    match DaemonClient::connect_with_timeout(&addr, Duration::from_secs(3)).await {
        Ok(mut client) => {
            let info = client.ping().await?;
            println!("DevLaunch Daemon v{}", info.version);
            println!("  Project:  {}", info.project_name);
            println!("  Uptime:   {}s", info.uptime_secs);
            println!(
                "  Services: {}/{} running",
                info.services_running, info.services_total
            );
        }
        Err(_) => {
            println!(
                "No daemon is running (cannot connect on port {}).",
                DEFAULT_DAEMON_PORT
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
