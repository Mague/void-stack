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

    /// Rename and/or move a registered project, migrating indexes, trust
    /// approval and git hooks — never re-indexing
    Edit {
        /// Current project name
        name: String,
        /// New project name
        #[arg(long = "name")]
        new_name: Option<String>,
        /// New project path (move the directory first, then run this)
        #[arg(long = "path")]
        new_path: Option<String>,
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
        /// Show the last 24 hours instead of `--days` (current session view)
        #[arg(long)]
        live: bool,
    },

    /// Index project codebase for semantic search (BAAI/bge-small-en-v1.5, local)
    #[cfg(feature = "vector")]
    Index {
        /// Project name
        project: String,
        /// Force re-index all files
        #[arg(long)]
        force: bool,
        /// Generate optimized .voidignore for semantic index quality
        #[arg(long)]
        generate_voidignore: bool,
        /// Git ref to diff against for change detection (e.g. "HEAD", "HEAD~1", "main").
        /// Faster and more accurate than mtime after checkout/stash/pull.
        #[arg(long)]
        git_base: Option<String>,
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

    /// Run Leiden community clustering over the semantic index
    #[cfg(feature = "vector")]
    Cluster {
        /// Project name
        project: String,
    },

    /// Register void-stack-mcp in installed MCP clients (Claude Desktop/Code, Cursor, Windsurf, Cline, VS Code)
    Setup {
        /// Print what would change without writing
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        /// Apply to all detected clients without prompting
        #[arg(long, default_value_t = false)]
        yes: bool,
        /// Path to the void-stack-mcp binary (auto-detected by default)
        #[arg(long)]
        mcp_path: Option<String>,
    },

    /// List dead-code candidates (zero-caller symbols)
    #[cfg(feature = "structural")]
    #[command(name = "dead-code")]
    DeadCode {
        /// Project name
        project: String,
    },

    /// Compact review payload for the current git diff
    #[cfg(feature = "structural")]
    Review {
        /// Project name
        project: String,
        /// Git ref to diff against (default: HEAD)
        #[arg(long)]
        git_base: Option<String>,
    },

    /// Suggest which tests cover the current git diff
    #[cfg(feature = "structural")]
    #[command(name = "suggest-tests")]
    SuggestTests {
        /// Project name
        project: String,
        /// Git ref to diff against (default: HEAD)
        #[arg(long)]
        git_base: Option<String>,
    },

    /// Build the structural call graph (tree-sitter) for a project
    #[cfg(feature = "structural")]
    #[command(name = "graph-build")]
    GraphBuild {
        /// Project name
        project: String,
        /// Force re-parse all files (ignore SHA cache)
        #[arg(long)]
        force: bool,
    },

    /// GraphRAG: semantic search + structural call-graph expansion
    #[cfg(all(feature = "vector", feature = "structural"))]
    Graphrag {
        /// Project name
        project: String,
        /// Natural language query
        query: String,
        /// BFS depth across the call graph (1 or 2 typical, max 3)
        #[arg(long, default_value_t = 2)]
        depth: u8,
        /// Also search related projects and surface shared symbols
        #[arg(long)]
        cross: bool,
    },

    /// Generate an interactive `graph.html` (self-contained, no CDN)
    #[command(name = "graph-html")]
    GraphHtml {
        /// Project name
        project: String,
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

    /// One-call session bootstrap: index/graph freshness, docs digest,
    /// current diff + impact radius, Doing tasks (compact markdown)
    Context {
        /// Project name
        project: String,
    },

    /// Kanban board stored as BOARD.md in the project repo
    Board {
        /// Project name (prints the board)
        project: Option<String>,
        #[command(subcommand)]
        action: Option<BoardAction>,
    },

    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum BoardAction {
    /// Add a task to the Backlog column
    Add {
        /// Project name
        project: String,
        /// Task title
        title: String,
        /// Priority (low, medium, high)
        #[arg(long)]
        prio: Option<String>,
        /// Tag (repeatable)
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// Move a task to another column
    Move {
        /// Project name
        project: String,
        /// Task id (e.g. VB-3)
        id: String,
        /// Target column (Backlog, Doing, Review, Done)
        column: String,
    },
    /// Move a task to Done
    Done {
        /// Project name
        project: String,
        /// Task id (e.g. VB-3)
        id: String,
    },
    /// Attach file paths or symbol names to a task
    Link {
        /// Project name
        project: String,
        /// Task id (e.g. VB-3)
        id: String,
        /// Files or symbols to link
        #[arg(required = true)]
        links: Vec<String>,
    },
    /// Archive old Done tasks to BOARD_ARCHIVE.md
    Archive {
        /// Project name
        project: String,
        /// Archive Done tasks older than this many days
        #[arg(long, default_value_t = 14)]
        days: i64,
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
        Commands::Edit {
            name,
            new_name,
            new_path,
        } => {
            commands::project::cmd_edit(name, new_name.as_deref(), new_path.as_deref())?;
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
            live,
        } => {
            commands::stats::run(*days, project.as_deref(), *json, *live)?;
        }
        #[cfg(feature = "vector")]
        Commands::Index {
            project,
            force,
            generate_voidignore,
            git_base,
        } => {
            if *generate_voidignore {
                commands::analysis::cmd_generate_voidignore(project)?;
            }
            commands::analysis::cmd_index(project, *force, git_base.as_deref())?;
        }
        #[cfg(feature = "vector")]
        Commands::Search {
            project,
            query,
            top_k,
        } => {
            commands::analysis::cmd_search(project, query, *top_k)?;
        }
        #[cfg(feature = "vector")]
        Commands::Cluster { project } => {
            commands::analysis::cmd_cluster(project)?;
        }
        Commands::Setup {
            dry_run,
            yes,
            mcp_path,
        } => {
            commands::setup::cmd_setup(*dry_run, *yes, mcp_path.as_deref())?;
        }
        #[cfg(feature = "structural")]
        Commands::DeadCode { project } => {
            commands::analysis::cmd_dead_code(project)?;
        }
        #[cfg(feature = "structural")]
        Commands::Review { project, git_base } => {
            commands::analysis::cmd_review(project, git_base.as_deref())?;
        }
        #[cfg(feature = "structural")]
        Commands::SuggestTests { project, git_base } => {
            commands::analysis::cmd_suggest_tests(project, git_base.as_deref())?;
        }
        #[cfg(feature = "structural")]
        Commands::GraphBuild { project, force } => {
            commands::analysis::cmd_graph_build(project, *force)?;
        }
        #[cfg(all(feature = "vector", feature = "structural"))]
        Commands::Graphrag {
            project,
            query,
            depth,
            cross,
        } => {
            if *cross {
                commands::analysis::cmd_graphrag_cross(project, query, *depth)?;
            } else {
                commands::analysis::cmd_graphrag(project, query, *depth)?;
            }
        }
        Commands::GraphHtml { project } => {
            commands::analysis::cmd_graph_html(project)?;
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
            )
            .await?;
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
            commands::analysis::cmd_audit(project, output.as_deref()).await?;
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
        Commands::Context { project } => {
            commands::context::cmd_context(project)?;
        }
        Commands::Board { project, action } => match action {
            Some(BoardAction::Add {
                project,
                title,
                prio,
                tags,
            }) => {
                commands::board::cmd_board_add(project, title, prio.as_deref(), tags)?;
            }
            Some(BoardAction::Move {
                project,
                id,
                column,
            }) => {
                commands::board::cmd_board_move(project, id, column)?;
            }
            Some(BoardAction::Done { project, id }) => {
                commands::board::cmd_board_move(project, id, "Done")?;
            }
            Some(BoardAction::Link { project, id, links }) => {
                commands::board::cmd_board_link(project, id, links)?;
            }
            Some(BoardAction::Archive { project, days }) => {
                commands::board::cmd_board_archive(project, *days)?;
            }
            None => match project {
                Some(p) => commands::board::cmd_board_list(p)?,
                None => anyhow::bail!(
                    "usage: void board <project> | void board <add|move|done|link|archive> ..."
                ),
            },
        },
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
