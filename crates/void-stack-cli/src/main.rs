mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
        /// Command to run (or Docker image like "postgres:16")
        command: String,
        /// Working directory (absolute path)
        #[arg(short = 'd', long)]
        dir: String,
        /// Target: windows, wsl, docker, ssh
        #[arg(short, long, default_value = "windows")]
        target: String,
        /// Docker port mappings (e.g., "5432:5432"). Repeatable.
        #[arg(long = "port", num_args = 1)]
        ports: Vec<String>,
        /// Docker volume mounts (e.g., "./data:/var/lib/data"). Repeatable.
        #[arg(long = "volume", num_args = 1)]
        volumes: Vec<String>,
        /// Extra docker run arguments (e.g., "--network=host"). Repeatable.
        #[arg(long = "docker-arg", num_args = 1)]
        docker_args: Vec<String>,
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
        /// Print the full diagram content to stdout (drawio XML or mermaid markdown)
        #[arg(long)]
        print_content: bool,
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

    /// Read any file from a project (blocked: .env, credentials, private keys)
    #[command(name = "read-file")]
    ReadFile {
        /// Project name
        project: String,
        /// Relative path to the file (e.g. "src/main.rs", "Cargo.toml")
        path: String,
    },

    /// Show captured logs for a running service
    Logs {
        /// Project name
        project: String,
        /// Service name
        service: String,
        /// Number of log lines to show (default: 100)
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
        /// Compact mode: filter noise, show only warnings/errors
        #[arg(long)]
        compact: bool,
        /// Raw output without any filtering
        #[arg(long)]
        raw: bool,
    },

    /// Show token savings statistics
    Stats {
        /// Filter by project name
        #[arg(short, long)]
        project: Option<String>,
        /// Number of days to include (default: 30)
        #[arg(short, long, default_value_t = 30)]
        days: u32,
        /// Output as JSON instead of table
        #[arg(long)]
        json: bool,
    },

    /// Index project codebase for semantic search (BAAI/bge-small-en-v1.5, local)
    #[cfg(feature = "vector")]
    Index {
        /// Project name
        project: String,
        /// Force re-index all files
        #[arg(long)]
        force: bool,
    },

    /// Semantic search across indexed codebase
    #[cfg(feature = "vector")]
    Search {
        /// Project name
        project: String,
        /// Natural language query
        query: String,
        /// Number of results (default: 5)
        #[arg(short, long, default_value_t = 5)]
        top_k: usize,
    },

    /// Generate a .claudeignore file optimized for the project's tech stack
    Claudeignore {
        /// Project name (case-insensitive)
        project: String,
        /// Preview without saving
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing .claudeignore without confirmation
        #[arg(long)]
        force: bool,
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
        Commands::Add {
            name,
            path,
            wsl,
            distro,
        } => {
            commands::project::cmd_add(name, path, *wsl, distro.as_deref())?;
        }
        Commands::AddService {
            project,
            name,
            command,
            dir,
            target,
            ports,
            volumes,
            docker_args,
        } => {
            commands::project::cmd_add_service(
                project,
                name,
                command,
                dir,
                target,
                ports,
                volumes,
                docker_args,
            )?;
        }
        Commands::Remove { name } => {
            commands::project::cmd_remove(name)?;
        }
        Commands::List => {
            commands::project::cmd_list()?;
        }
        Commands::Scan { path, wsl, distro } => {
            commands::project::cmd_scan(path, *wsl, distro.as_deref());
        }
        Commands::ReadFile { project, path } => {
            commands::project::cmd_read_file(project, path)?;
        }
        Commands::Logs {
            project,
            service,
            lines,
            compact,
            raw,
        } => {
            commands::service::cmd_logs(
                cli.daemon, cli.port, project, service, *lines, *compact, *raw,
            )
            .await?;
        }
        Commands::Stats {
            project,
            days,
            json,
        } => {
            commands::project::cmd_stats(project.as_deref(), *days, *json)?;
        }
        #[cfg(feature = "vector")]
        Commands::Index { project, force } => {
            commands::analysis::cmd_index(project, *force)?;
        }
        #[cfg(feature = "vector")]
        Commands::Search {
            project,
            query,
            top_k,
        } => {
            commands::analysis::cmd_search(project, query, *top_k)?;
        }
        Commands::Claudeignore {
            project,
            dry_run,
            force,
        } => {
            commands::project::cmd_claudeignore(project, *dry_run, *force)?;
        }
        Commands::Init { path } => {
            commands::project::cmd_init(path)?;
        }
        Commands::Start { project, service } => {
            commands::service::cmd_start(cli.daemon, cli.port, project, service.as_deref()).await?;
        }
        Commands::Stop { project, service } => {
            commands::service::cmd_stop(cli.daemon, cli.port, project, service.as_deref()).await?;
        }
        Commands::Status { project } => {
            commands::service::cmd_status(project.as_deref()).await?;
        }
        Commands::Check { project } => {
            commands::deps::cmd_check(project).await?;
        }
        Commands::Analyze {
            project,
            output,
            service,
            label,
            compare,
            cross_project,
            best_practices,
            bp_only,
        } => {
            commands::analysis::cmd_analyze(
                project,
                output.as_deref(),
                service.as_deref(),
                label.as_deref(),
                *compare,
                *cross_project,
                *best_practices || *bp_only,
                *bp_only,
            )?;
        }
        Commands::Diagram {
            project,
            output,
            format,
            print_content,
        } => {
            commands::analysis::cmd_diagram(project, output.as_deref(), format, *print_content)?;
        }
        Commands::Audit { project, output } => {
            commands::analysis::cmd_audit(project, output.as_deref())?;
        }
        Commands::Suggest {
            project,
            model,
            service,
            raw,
        } => {
            commands::analysis::cmd_suggest(project, model.as_deref(), service.as_deref(), *raw)
                .await?;
        }
        Commands::Docker {
            project,
            generate_dockerfile,
            generate_compose,
            save,
        } => {
            commands::docker::cmd_docker(project, *generate_dockerfile, *generate_compose, *save)?;
        }
        Commands::Daemon { action } => match action {
            DaemonAction::Start { project, port } => {
                commands::daemon::cmd_daemon_start(project, *port).await?;
            }
            DaemonAction::Stop => {
                commands::daemon::cmd_daemon_stop().await?;
            }
            DaemonAction::Status => {
                commands::daemon::cmd_daemon_status().await?;
            }
        },
    }

    Ok(())
}
