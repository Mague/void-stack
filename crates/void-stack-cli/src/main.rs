use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use void_stack_core::backend::ServiceBackend;
use void_stack_core::config;
use void_stack_core::global_config::{
    self, default_command_for_dir, find_project, load_global_config, remove_project,
    save_global_config, scan_subprojects,
};
use void_stack_core::manager::ProcessManager;
use void_stack_core::model::*;
use void_stack_proto::client::DaemonClient;

const DEFAULT_DAEMON_PORT: u16 = 50051;

#[derive(Parser)]
#[command(name = "void", about = "Unified dev service launcher & monitor")]
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
        /// WSL distro name (default: auto-detect default distro)
        #[arg(long)]
        distro: Option<String>,
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

    /// Analyze code: dependency graph, architecture patterns, anti-patterns, complexity
    Analyze {
        /// Project name
        project: String,
        /// Output file path (default: <project_dir>/void-stack-analysis.md)
        #[arg(short, long)]
        output: Option<String>,
        /// Specific service to analyze (omit for all)
        #[arg(short, long)]
        service: Option<String>,
        /// Optional label for the snapshot (e.g., git tag, version)
        #[arg(long)]
        label: Option<String>,
        /// Compare against previous analysis snapshot
        #[arg(long)]
        compare: bool,
        /// Detect dependencies between registered projects
        #[arg(long)]
        cross_project: bool,
        /// Run best practices analysis (ruff, clippy, golangci-lint, react-doctor, dart analyze)
        #[arg(long)]
        best_practices: bool,
        /// Only run best practices analysis (skip architecture analysis)
        #[arg(long)]
        bp_only: bool,
    },

    /// Generate architecture/API/DB diagrams for a project
    Diagram {
        /// Project name
        project: String,
        /// Output file path (default: <project_dir>/void-stack-diagrams.{md,drawio})
        #[arg(short, long)]
        output: Option<String>,
        /// Format: mermaid or drawio (default: drawio)
        #[arg(short, long, default_value = "drawio")]
        format: String,
    },

    /// Run security audit: vulnerabilities, secrets, insecure configs
    Audit {
        /// Project name
        project: String,
        /// Output file path (default: <project_dir>/void-stack-audit.md)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Scan a directory and show what void detects
    Scan {
        /// Path to scan
        #[arg(default_value = ".")]
        path: String,
        /// Scan inside WSL (path is a Linux path)
        #[arg(long)]
        wsl: bool,
        /// WSL distro name (default: auto-detect default distro)
        #[arg(long)]
        distro: Option<String>,
    },

    /// Analyze Docker artifacts and generate Dockerfiles/compose
    Docker {
        /// Project name
        project: String,
        /// Generate a Dockerfile if missing
        #[arg(long)]
        generate_dockerfile: bool,
        /// Generate a docker-compose.yml
        #[arg(long)]
        generate_compose: bool,
        /// Save generated files to project directory
        #[arg(long)]
        save: bool,
    },

    /// Generate AI-powered refactoring suggestions using Ollama (local LLM)
    Suggest {
        /// Project name
        project: String,
        /// Override the default AI model (e.g., "llama3.2", "qwen2.5-coder:7b")
        #[arg(short, long)]
        model: Option<String>,
        /// Specific service to analyze (omit for all)
        #[arg(short, long)]
        service: Option<String>,
        /// Show raw LLM response instead of parsed suggestions
        #[arg(long)]
        raw: bool,
    },

    /// Initialize a void-stack.toml in a directory (legacy/local mode)
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
        Commands::Add { name, path, wsl, distro } => cmd_add(name, path, *wsl, distro.as_deref())?,
        Commands::AddService { project, name, command, dir, target } => {
            cmd_add_service(project, name, command, dir, target)?;
        }
        Commands::Remove { name } => cmd_remove(name)?,
        Commands::List => cmd_list()?,
        Commands::Check { project } => cmd_check(project).await?,
        Commands::Audit { project, output } => cmd_audit(project, output.as_deref())?,
        Commands::Analyze { project, output, service, label, compare, cross_project, best_practices, bp_only } => cmd_analyze(project, output.as_deref(), service.as_deref(), label.as_deref(), *compare, *cross_project, *best_practices || *bp_only, *bp_only)?,
        Commands::Diagram { project, output, format } => cmd_diagram(project, output.as_deref(), format)?,
        Commands::Scan { path, wsl, distro } => cmd_scan(path, *wsl, distro.as_deref()),
        Commands::Docker { project, generate_dockerfile, generate_compose, save } => {
            cmd_docker(project, *generate_dockerfile, *generate_compose, *save)?;
        }
        Commands::Suggest { project, model, service, raw } => {
            cmd_suggest(project, model.as_deref(), service.as_deref(), *raw).await?;
        }
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

fn cmd_add(name: &str, path: &str, wsl: bool, distro: Option<&str>) -> Result<()> {
    let mut config = load_global_config()?;

    if find_project(&config, name).is_some() {
        bail!("Project '{}' already exists. Use 'void remove {}' first.", name, name);
    }

    let default_target = if wsl { Target::Wsl } else { Target::Windows };

    // Resolve path: for WSL → UNC path, for Windows → canonicalize
    let scan_path = if wsl {
        resolve_wsl_path(path, distro)
    } else {
        std::fs::canonicalize(path)
            .with_context(|| format!("Path not found: {}", path))?
    };

    let detected = scan_subprojects(&scan_path);
    let services: Vec<Service> = if detected.is_empty() {
        println!("No services auto-detected. Add them manually with 'void add-service'.");
        vec![]
    } else {
        println!("Detected {} service(s) in {}:", detected.len(), scan_path.display());
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
    };

    let project_path = scan_path.to_string_lossy().to_string();

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
    println!("\nProject '{}' added. Edit services with 'void add-service' or edit the config directly.", name);
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
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found. Use 'void add' first.", project_name))?;

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
        println!("Add one with: void add <name> <path>");
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

/// Resolve a WSL path to a UNC path that works with Windows std::fs.
/// Handles:
///   - Linux paths: /home/user/project → \\wsl.localhost\<distro>\home\user\project
///   - UNC paths: \\wsl.localhost\Ubuntu\... → passed through (stripped of \\?\UNC\ if present)
///   - Git Bash mangled: C:\Program Files\Git\home\user\... → \\wsl.localhost\<distro>\home\user\...
fn resolve_wsl_path(path: &str, distro: Option<&str>) -> std::path::PathBuf {
    use void_stack_core::runner::local::is_wsl_unc_path;

    let d = distro
        .map(|s| s.to_string())
        .or_else(detect_default_wsl_distro)
        .unwrap_or_else(|| "Ubuntu".to_string());

    // Already a proper UNC WSL path
    if is_wsl_unc_path(path) {
        return std::path::PathBuf::from(path);
    }

    // Handle \\?\UNC\wsl.localhost\... (Git Bash converts // to \\?\UNC\)
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\wsl.localhost\")
        .or_else(|| path.strip_prefix(r"\\?\UNC\wsl$\"))
    {
        return std::path::PathBuf::from(format!(r"\\wsl.localhost\{}", rest));
    }

    // Git Bash mangles /home/... to C:\Program Files\Git\home\...
    // Detect this pattern: drive letter + path containing \home\ or similar Linux paths
    let normalized = path.replace('/', r"\");
    for linux_root in &[r"\home\", r"\opt\", r"\usr\", r"\var\", r"\tmp\", r"\etc\", r"\root\", r"\mnt\"] {
        if let Some(pos) = normalized.find(linux_root) {
            let linux_part = &normalized[pos..].replace('\\', "/");
            return std::path::PathBuf::from(format!(
                r"\\wsl.localhost\{}{}",
                d,
                linux_part.replace('/', r"\")
            ));
        }
    }

    // Pure Linux path starting with /
    if path.starts_with('/') {
        return std::path::PathBuf::from(format!(
            r"\\wsl.localhost\{}{}",
            d,
            path.replace('/', r"\")
        ));
    }

    // Fallback: use as-is
    std::path::PathBuf::from(path)
}

/// Detect the default WSL distribution name.
fn detect_default_wsl_distro() -> Option<String> {
    let output = std::process::Command::new("wsl")
        .args(["--list", "--quiet"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // WSL outputs UTF-16LE on Windows
    let text = String::from_utf16_lossy(
        &output
            .stdout
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<u16>>(),
    );

    // First non-empty line is the default distro
    text.lines()
        .map(|l| l.trim().trim_matches('\0').to_string())
        .find(|l| !l.is_empty())
}

fn cmd_scan(path: &str, wsl: bool, distro: Option<&str>) {
    let scan_path = if wsl {
        resolve_wsl_path(path, distro)
    } else {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.into())
    };

    println!("Scanning {}...\n", scan_path.display());
    let detected = scan_subprojects(&scan_path);
    if detected.is_empty() {
        println!("No projects detected.");
    } else {
        for (name, sub_path, pt) in &detected {
            println!("  {:?} → {} ({})", pt, name, sub_path.display());
        }
        if wsl {
            println!(
                "\nUse 'void add <name> {} --wsl' to register this project.",
                path
            );
        } else {
            println!(
                "\nUse 'void add <name> {}' to register this project.",
                scan_path.display()
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
    println!("Created void-stack.toml ({:?} project detected)", project_type);
    Ok(())
}

// ── Security Audit ──────────────────────────────────────────

fn cmd_audit(project_name: &str, output: Option<&str>) -> Result<()> {
    use void_stack_core::audit;
    use void_stack_core::runner::local::strip_win_prefix;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let clean_path = strip_win_prefix(&project.path);
    println!("Running security audit for '{}'...\n", project.name);

    let result = audit::audit_project(&project.name, Path::new(&clean_path));

    // Print summary
    if result.summary.total == 0 {
        println!("  ✅ No se encontraron problemas de seguridad.\n");
    } else {
        println!("  Hallazgos:");
        if result.summary.critical > 0 {
            println!("    🔴 Critical: {}", result.summary.critical);
        }
        if result.summary.high > 0 {
            println!("    🟠 High:     {}", result.summary.high);
        }
        if result.summary.medium > 0 {
            println!("    🟡 Medium:   {}", result.summary.medium);
        }
        if result.summary.low > 0 {
            println!("    🔵 Low:      {}", result.summary.low);
        }
        if result.summary.info > 0 {
            println!("    ℹ️  Info:     {}", result.summary.info);
        }
        println!("    Total:       {}", result.summary.total);
        println!("    Risk Score:  {:.0}/100\n", result.summary.risk_score);

        // Print findings
        for finding in &result.findings {
            let icon = match finding.severity {
                audit::Severity::Critical => "🔴",
                audit::Severity::High => "🟠",
                audit::Severity::Medium => "🟡",
                audit::Severity::Low => "🔵",
                audit::Severity::Info => "ℹ️",
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

// ── Analyze ─────────────────────────────────────────────────

fn cmd_analyze(
    project_name: &str,
    output: Option<&str>,
    service_filter: Option<&str>,
    label: Option<&str>,
    do_compare: bool,
    do_cross_project: bool,
    do_best_practices: bool,
    bp_only: bool,
) -> Result<()> {
    use void_stack_core::runner::local::strip_win_prefix;
    use void_stack_core::analyzer::history;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    // Collect directories to analyze
    let mut dirs: Vec<(String, std::path::PathBuf)> = Vec::new();

    match service_filter {
        Some(svc_name) => {
            let svc = project.services.iter()
                .find(|s| s.name.eq_ignore_ascii_case(svc_name))
                .ok_or_else(|| anyhow::anyhow!("Service '{}' not found in project.", svc_name))?;
            let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
            let clean = strip_win_prefix(dir);
            dirs.push((svc.name.clone(), Path::new(&clean).to_path_buf()));
        }
        None => {
            for svc in &project.services {
                let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
                let clean = strip_win_prefix(dir);
                dirs.push((svc.name.clone(), Path::new(&clean).to_path_buf()));
            }
            if dirs.is_empty() {
                let clean = strip_win_prefix(&project.path);
                dirs.push((project.name.clone(), Path::new(&clean).to_path_buf()));
            }
        }
    }

    let mut full_doc = String::new();
    let mut named_results: Vec<(String, void_stack_core::analyzer::AnalysisResult)> = Vec::new();
    let project_path_str = strip_win_prefix(&project.path);

    if bp_only {
        // Skip architecture analysis — go straight to best practices
    } else {

    for (svc_name, dir) in &dirs {
        println!("Analyzing {}...", svc_name);

        match void_stack_core::analyzer::analyze_project(dir) {
            Some(result) => {
                let doc = void_stack_core::analyzer::generate_docs(&result, svc_name);
                full_doc.push_str(&doc);
                full_doc.push_str("\n\n---\n\n");

                // Print summary to console
                println!("  Pattern: {} ({:.0}% confidence)", result.architecture.detected_pattern, result.architecture.confidence * 100.0);
                println!("  Modules: {}", result.graph.modules.len());
                let total_loc: usize = result.graph.modules.iter().map(|m| m.loc).sum();
                println!("  LOC: {}", total_loc);
                println!("  External deps: {}", result.graph.external_deps.len());

                // Complexity summary
                if let Some(cx) = &result.complexity {
                    let all_funcs: Vec<_> = cx.iter()
                        .flat_map(|(_, fc)| fc.functions.iter())
                        .collect();
                    if !all_funcs.is_empty() {
                        let max = all_funcs.iter().max_by_key(|f| f.complexity).unwrap();
                        let complex_count = all_funcs.iter().filter(|f| f.complexity >= 10).count();
                        println!("  Complexity: max {} ({}), {} complex functions",
                            max.complexity, max.name, complex_count);
                    }
                }

                if !result.architecture.anti_patterns.is_empty() {
                    println!("  Anti-patterns: {}", result.architecture.anti_patterns.len());
                    for ap in &result.architecture.anti_patterns {
                        println!("    [{:?}] {}: {}", ap.severity, ap.kind, ap.description);
                    }
                } else {
                    println!("  No anti-patterns detected.");
                }
                if let Some(cov) = &result.coverage {
                    println!("  Coverage: {:.1}% ({}/{} lines) [{}]",
                        cov.coverage_percent, cov.covered_lines, cov.total_lines, cov.tool);
                }
                println!();

                named_results.push((svc_name.clone(), result));
            }
            None => {
                println!("  Could not detect language for {}", dir.display());
            }
        }
    }

    // Save snapshot for debt tracking
    let project_path = Path::new(&project_path_str);
    if !named_results.is_empty() {
        let snapshot = history::create_snapshot(&named_results, label.map(|s| s.to_string()));

        // Compare against previous if requested
        if do_compare {
            if let Some(previous) = history::load_latest(project_path) {
                let comparison = history::compare(&previous, &snapshot);
                let comp_md = history::comparison_markdown(&comparison);
                full_doc.push_str(&comp_md);

                println!("Debt trend: {} (vs {})",
                    comparison.overall_trend,
                    previous.timestamp.format("%Y-%m-%d %H:%M"));
                for svc in &comparison.services {
                    println!("  {} — LOC: {}, anti-patterns: {}, complexity: {}, trend: {}",
                        svc.name,
                        format_delta(svc.loc_delta),
                        format_delta_i32_cli(svc.antipattern_delta),
                        format_delta_f32_cli(svc.complexity_delta),
                        svc.trend);
                }
                println!();
            } else {
                println!("No previous snapshot found for comparison.\n");
            }
        }

        // Save current snapshot
        if let Err(e) = history::save_snapshot(project_path, &snapshot) {
            eprintln!("Warning: could not save analysis snapshot: {}", e);
        }
    }

    // Cross-project analysis
    if do_cross_project && !named_results.is_empty() {
        let mut all_analysis = HashMap::new();
        all_analysis.insert(project.name.clone(), named_results.iter().map(|(n, r)| (n.clone(), r.clone())).collect());

        // Analyze other projects too for cross-referencing
        for other in &config.projects {
            if other.name.eq_ignore_ascii_case(&project.name) {
                continue;
            }
            let mut other_results = Vec::new();
            for svc in &other.services {
                let dir = svc.working_dir.as_deref().unwrap_or(&other.path);
                let clean = strip_win_prefix(dir);
                if let Some(result) = void_stack_core::analyzer::analyze_project(Path::new(&clean)) {
                    other_results.push((svc.name.clone(), result));
                }
            }
            if !other_results.is_empty() {
                all_analysis.insert(other.name.clone(), other_results);
            }
        }

        let cross = void_stack_core::analyzer::analyze_cross_project(&config.projects, &all_analysis);
        if !cross.links.is_empty() {
            let cross_md = void_stack_core::analyzer::cross_project::cross_project_markdown(&cross);
            full_doc.push_str(&cross_md);

            println!("Cross-project dependencies:");
            for link in &cross.links {
                println!("  {} ({}) --> {} via '{}'",
                    link.from_project, link.from_service, link.to_project, link.via_dependency);
            }
            println!();
        }
    }

    } // end if !bp_only

    // Best practices analysis
    if do_best_practices {
        use void_stack_core::analyzer::best_practices;
        use void_stack_core::analyzer::best_practices::report::generate_best_practices_markdown;

        println!("Running best practices analysis...");
        let bp_result = best_practices::analyze_best_practices(Path::new(&project_path_str));

        // Print summary
        if bp_result.tools_used.is_empty() {
            println!("  No applicable linting tools found.");
        } else {
            println!("  Overall Score: {:.0}/100", bp_result.overall_score);
            println!("  Tools: {}", bp_result.tools_used.join(", "));
            let important = bp_result.findings.iter().filter(|f| f.severity == best_practices::BpSeverity::Important).count();
            let warnings = bp_result.findings.iter().filter(|f| f.severity == best_practices::BpSeverity::Warning).count();
            let suggestions = bp_result.findings.iter().filter(|f| f.severity == best_practices::BpSeverity::Suggestion).count();
            println!("  Findings: {} important, {} warnings, {} suggestions", important, warnings, suggestions);
            for ts in &bp_result.tool_scores {
                let native = ts.native_score.map(|n| format!(" (native: {:.0})", n)).unwrap_or_default();
                println!("    {} — score: {:.0}/100, {} findings{}", ts.tool, ts.score, ts.finding_count, native);
            }
        }
        println!();

        let bp_md = generate_best_practices_markdown(&bp_result);
        full_doc.push_str(&bp_md);
    }

    if !full_doc.is_empty() {
        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(&project.path);
                format!("{}/void-stack-analysis.md", dir)
            }
        };
        std::fs::write(&path, &full_doc)?;
        println!("Analysis saved to {}", path);
    }

    Ok(())
}

fn format_delta(v: i64) -> String {
    if v > 0 { format!("+{}", v) }
    else if v < 0 { format!("{}", v) }
    else { "=".to_string() }
}

fn format_delta_i32_cli(v: i32) -> String {
    if v > 0 { format!("+{}", v) }
    else if v < 0 { format!("{}", v) }
    else { "=".to_string() }
}

fn format_delta_f32_cli(v: f32) -> String {
    if v > 0.1 { format!("+{:.1}", v) }
    else if v < -0.1 { format!("{:.1}", v) }
    else { "=".to_string() }
}

// ── Diagram ──────────────────────────────────────────────────

fn cmd_diagram(project_name: &str, output: Option<&str>, format: &str) -> Result<()> {
    use void_stack_core::runner::local::strip_win_prefix;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    let is_drawio = format.eq_ignore_ascii_case("drawio") || format.eq_ignore_ascii_case("draw.io");

    if is_drawio {
        let content = void_stack_core::diagram::drawio::generate_all(project);
        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(&project.path);
                format!("{}/void-stack-diagrams.drawio", dir)
            }
        };
        std::fs::write(&path, &content)?;
        println!("Draw.io diagram saved to {}", path);
    } else {
        // Mermaid format
        let diagrams = void_stack_core::diagram::generate_all(project);
        let mut content = String::new();
        content.push_str(&format!("# {} — Architecture\n\n", project.name));
        content.push_str("## Service Architecture\n\n");
        content.push_str(&diagrams.architecture);
        content.push_str("\n\n");

        if let Some(api) = &diagrams.api_routes {
            content.push_str("## API Routes\n\n");
            content.push_str(api);
            content.push_str("\n\n");
        }

        if let Some(db) = &diagrams.db_models {
            content.push_str("## Database Models\n\n");
            content.push_str(db);
            content.push_str("\n\n");
        }

        if !diagrams.warnings.is_empty() {
            content.push_str("## Advertencias\n\n");
            for w in &diagrams.warnings {
                content.push_str(&format!("- {}\n", w));
            }
            content.push_str("\n");

            for w in &diagrams.warnings {
                println!("  Warning: {}", w);
            }
        }

        let path = match output {
            Some(p) => p.to_string(),
            None => {
                let dir = strip_win_prefix(&project.path);
                format!("{}/void-stack-diagrams.md", dir)
            }
        };
        std::fs::write(&path, &content)?;
        println!("Mermaid diagrams saved to {}", path);
    }

    Ok(())
}

// ── Check dependencies ───────────────────────────────────────

async fn cmd_check(project_name: &str) -> Result<()> {
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

// ── Start ────────────────────────────────────────────────────

async fn cmd_start(cli: &Cli, project_name: &str, service: Option<&str>) -> Result<()> {
    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found. Use 'void list' to see available projects.", project_name))?
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
                                if hint.is_empty() { String::new() } else { format!(" (fix: {})", hint) },
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
                println!("No projects registered. Use 'void add <name> <path>'.");
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
        "  void-daemon start -p \"{}\" --port {}",
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
            println!("VoidStack Daemon v{}", info.version);
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

// ── Docker Intelligence ──

fn cmd_docker(project_name: &str, gen_dockerfile: bool, gen_compose: bool, save: bool) -> Result<()> {
    use void_stack_core::docker;
    use void_stack_core::runner::local::strip_win_prefix;

    let config = load_global_config()?;
    let proj = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Proyecto '{}' no encontrado", project_name))?;
    let clean_path = strip_win_prefix(&proj.path);
    let project_path = Path::new(&clean_path);

    // 1. Analyze existing Docker artifacts
    let analysis = docker::analyze_docker(project_path);

    println!("\n  Docker Analysis: {}", proj.name);
    println!("  {}", "─".repeat(40));

    if analysis.has_dockerfile {
        println!("  ✅ Dockerfile encontrado");
        if let Some(ref df) = analysis.dockerfile {
            for (i, stage) in df.stages.iter().enumerate() {
                let name = stage.name.as_deref().unwrap_or("(unnamed)");
                println!("     Stage {}: {} ({})", i, stage.base_image, name);
            }
            if !df.exposed_ports.is_empty() {
                println!("     Ports: {:?}", df.exposed_ports);
            }
            if let Some(ref cmd) = df.cmd {
                println!("     CMD: {}", cmd);
            }
        }
    } else {
        println!("  ⚠ No Dockerfile");
    }

    if analysis.has_compose {
        println!("  ✅ docker-compose encontrado");
        if let Some(ref compose) = analysis.compose {
            for svc in &compose.services {
                let ports: Vec<String> = svc.ports.iter().map(|p| format!("{}:{}", p.host, p.container)).collect();
                let ports_str = if ports.is_empty() { String::new() } else { format!(" [{}]", ports.join(", ")) };
                let img = svc.image.as_deref().unwrap_or("build");
                println!("     {} ({}) → {}{}", svc.name, svc.kind, img, ports_str);
            }
        }
    } else {
        println!("  ⚠ No docker-compose");
    }

    // Terraform
    if !analysis.terraform.is_empty() {
        println!("\n  ── Terraform ({} recursos) ──", analysis.terraform.len());
        for res in &analysis.terraform {
            let details = if res.details.is_empty() {
                String::new()
            } else {
                format!(" ({})", res.details.join(", "))
            };
            println!("     [{}] {} \"{}\" → {}{}", res.provider, res.resource_type, res.name, res.kind, details);
        }
    }

    // Kubernetes
    if !analysis.kubernetes.is_empty() {
        println!("\n  ── Kubernetes ({} recursos) ──", analysis.kubernetes.len());
        for res in &analysis.kubernetes {
            let ns = res.namespace.as_deref().unwrap_or("default");
            let images = if res.images.is_empty() { String::new() } else { format!(" images=[{}]", res.images.join(", ")) };
            let ports = if res.ports.is_empty() { String::new() } else { format!(" ports={:?}", res.ports) };
            let replicas = res.replicas.map(|r| format!(" x{}", r)).unwrap_or_default();
            println!("     {}: {} (ns={}){}{}{}",
                res.kind, res.name, ns, replicas, images, ports);
        }
    }

    // Helm
    if let Some(ref chart) = analysis.helm {
        println!("\n  ── Helm: {} v{} ──", chart.name, chart.version);
        if !chart.dependencies.is_empty() {
            for dep in &chart.dependencies {
                println!("     dep: {} ({}) → {}", dep.name, dep.version, dep.repository);
            }
        }
    }

    // 2. Generate Dockerfile
    if gen_dockerfile && !analysis.has_dockerfile {
        let project_type = config::detect_project_type(project_path);
        if let Some(content) = docker::generate_dockerfile::generate(project_path, project_type) {
            println!("\n  ── Dockerfile generado ──\n");
            for line in content.lines() {
                println!("  {}", line);
            }
            if save {
                let out = project_path.join("Dockerfile");
                std::fs::write(&out, &content)?;
                println!("\n  ✅ Guardado en {}", out.display());
            }
        } else {
            println!("\n  ⚠ No se pudo generar Dockerfile para tipo {:?}", config::detect_project_type(project_path));
        }
    } else if gen_dockerfile && analysis.has_dockerfile {
        println!("\n  ℹ Dockerfile ya existe, no se sobreescribe");
    }

    // 3. Generate docker-compose.yml
    if gen_compose {
        let content = docker::generate_compose::generate(&proj, project_path);
        println!("\n  ── docker-compose.yml generado ──\n");
        for line in content.lines() {
            println!("  {}", line);
        }
        if save {
            let out = project_path.join("docker-compose.yml");
            std::fs::write(&out, &content)?;
            println!("\n  ✅ Guardado en {}", out.display());
        }
    }

    if !gen_dockerfile && !gen_compose {
        println!("\n  Usa --generate-dockerfile y/o --generate-compose para generar archivos");
    }

    println!();
    Ok(())
}

// ── AI Suggestions ──────────────────────────────────────────

async fn cmd_suggest(project_name: &str, model_override: Option<&str>, service_filter: Option<&str>, raw: bool) -> Result<()> {
    use void_stack_core::ai;
    use void_stack_core::runner::local::strip_win_prefix;

    let config = load_global_config()?;
    let project = find_project(&config, project_name)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found.", project_name))?;

    // Load AI config
    let mut ai_config = ai::load_ai_config().unwrap_or_default();
    if let Some(model) = model_override {
        ai_config.model = model.to_string();
    }

    println!("Analizando proyecto '{}'...\n", project.name);

    // Collect analysis results
    let services: Vec<_> = match service_filter {
        Some(svc_name) => {
            project.services.iter()
                .filter(|s| s.name.eq_ignore_ascii_case(svc_name))
                .collect()
        }
        None => project.services.iter().collect(),
    };

    let mut analysis = None;
    for svc in &services {
        let dir = svc.working_dir.as_deref().unwrap_or(&project.path);
        let clean = strip_win_prefix(dir);
        let path = std::path::Path::new(&clean);
        if let Some(result) = void_stack_core::analyzer::analyze_project(path) {
            analysis = Some(result);
            break; // Use first analyzable service
        }
    }

    let analysis = analysis
        .ok_or_else(|| anyhow::anyhow!("No se pudo analizar el proyecto (sin archivos fuente detectados)"))?;

    println!("Generando sugerencias con {} ({})...\n", ai_config.provider_name(), ai_config.model);

    match ai::suggest(&ai_config, &analysis, &project.name).await {
        Ok(result) => {
            if raw {
                println!("{}", result.raw_response);
            } else {
                println!("Modelo: {}\n", result.model_used);
                if result.suggestions.is_empty() {
                    println!("  No se generaron sugerencias estructuradas.");
                    println!("\nRespuesta completa:\n{}", result.raw_response);
                } else {
                    for (i, s) in result.suggestions.iter().enumerate() {
                        let priority_icon = match s.priority {
                            ai::SuggestionPriority::Critical => "!!",
                            ai::SuggestionPriority::High => "! ",
                            ai::SuggestionPriority::Medium => "- ",
                            ai::SuggestionPriority::Low => "  ",
                        };
                        println!("{}. {} [{}] {}", i + 1, priority_icon, s.category, s.title);
                        println!("   {}", s.description);
                        if !s.affected_files.is_empty() {
                            println!("   Archivos: {}", s.affected_files.join(", "));
                        }
                        println!();
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error de AI: {}\n", e);
            println!("Mostrando contexto de análisis que puedes usar con tu asistente AI:\n");
            let context = ai::build_context(&analysis, &project.name);
            println!("{}", context);
        }
    }

    Ok(())
}
