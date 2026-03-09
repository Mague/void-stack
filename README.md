# Void Stack

**Got 10 projects with backends, frontends, workers, and databases — and you can't remember how to start any of them?**

Void Stack fixes that. One command to start your entire dev stack — backend, frontend, database, workers — on Windows, WSL, or Docker. No memorizing ports, no opening 5 terminals, no reading outdated READMEs.

```bash
void add my-app F:\projects\my-app    # Auto-detects services
void start my-app                      # Starts everything
```

That's it. Void Stack scans your project, detects which frameworks you're using (FastAPI, Vite, Express, Django...), generates the right commands, and runs them. If there's a Python venv, it finds it. If `node_modules` is missing, it tells you.

> **High Performance** — Built with Rust. Zero runtime overhead, instant startup, minimal memory footprint.

> **Agentic Workflow** — MCP server with 20+ tools lets Claude Desktop / Claude Code manage your services, analyze code, and audit security autonomously.

> **Cloud-Native Roadmap** — Deploy to Vercel, DigitalOcean, and more from the same config (coming soon).

**[Leer en español](README.es.md)** | **[void-stack.dev](https://void-stack.dev)**

<!-- TODO: Add screenshot/GIF of TUI and Desktop here -->

## Interfaces

Void Stack has **4 interfaces** — use whichever you prefer:

| Interface | Description |
|-----------|-------------|
| **CLI** (`void.exe`) | Fast commands from terminal |
| **TUI** (`void-tui.exe`) | Interactive terminal dashboard with live logs |
| **Desktop** (`void-desktop.exe`) | Desktop app with GUI (Tauri + React) |
| **MCP Server** (`void-mcp.exe`) | Integration with Claude Desktop / Claude Code |

## End-to-end example: FastAPI + React in 30 seconds

Say you have a project with a FastAPI backend and a React frontend:

```
my-app/
├── backend/       # FastAPI with venv
│   ├── main.py    # from fastapi import FastAPI
│   └── .venv/
├── frontend/      # React with Vite
│   ├── package.json
│   └── src/
└── .env
```

```bash
# 1. Register the project (scans and detects services)
void add my-app F:\projects\my-app

# Void Stack detects:
#   ✓ backend  → uvicorn main:app --host 0.0.0.0 --port 8000
#   ✓ frontend → npm run dev
#   ✓ .venv    → auto-resolves python to virtualenv

# 2. Check dependencies
void check my-app
#   ✅ Python 3.11 (venv detected)
#   ✅ Node 20.x (node_modules up to date)
#   ✅ .env complete vs .env.example

# 3. Start everything
void start my-app
#   [backend]  → http://localhost:8000
#   [frontend] → http://localhost:5173

# 4. Or open the interactive dashboard
void-tui my-app
```

## Installation

### Prerequisites

- **Rust** (rustc + cargo). If you don't have it:
  ```bash
  # Windows (winget)
  winget install Rustlang.Rust.MSVC

  # Or from https://rustup.rs
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

- **Protobuf compiler** (for the gRPC daemon):
  ```bash
  winget install Google.Protobuf
  ```

### Build

```bash
git clone https://github.com/mague/void-stack.git
cd void-stack
cargo build --release

# Binaries in target/release/
#   void.exe           — CLI
#   void-tui.exe       — Terminal dashboard
#   void-desktop.exe   — Desktop app (Tauri)
#   void-daemon.exe    — gRPC daemon
#   void-mcp.exe       — MCP server for AI
```

### Desktop (Tauri)

```bash
cd crates/void-stack-desktop/frontend
npm install
npm run build
cd ..
cargo tauri build
# Generates MSI/NSIS installer in target/release/bundle/
```

## Features

- **Multi-service** — Start/stop all services together or individually
- **Cross-platform** — Windows (`cmd`), WSL (`bash`), Docker, SSH (future)
- **Auto-detection** — Scans directories and identifies Python, Node, Rust, Go, Flutter, Docker
- **Smart commands** — Detects FastAPI, Flask, Django, Vite, Next.js, Express and generates the right command
- **Pre-launch hooks** — Creates venvs, installs deps, runs builds automatically
- **Dependency checking** — Verifies Python, Node, CUDA, Ollama, Docker, Rust, `.env`
- **Live logs** — Stdout/stderr from all services with automatic URL detection
- **Diagrams** — Generates Mermaid and Draw.io from project structure (architecture, API routes with Swagger/OpenAPI enrichment, internal/external API separation, gRPC services, DB models with FK-proximity layout)
- **Code analysis** — Dependency graphs, anti-patterns, cyclomatic complexity, coverage
- **Best practices** — Native linters (react-doctor, ruff, clippy, golangci-lint, dart analyze) with unified scoring
- **Technical debt** — Metric snapshots with trend comparison
- **AI integration** — MCP server with 20+ tools for Claude Desktop / Claude Code
- **Disk space scanner** — Scan and clean project deps (node_modules, venv, target) and global caches (npm, pip, Cargo, Ollama, HuggingFace, LM Studio)
- **Desktop GUI** — Tauri app with cyberpunk mission-control aesthetic, visual hierarchy (KPI cards, glow effects, severity gradients), services, logs, dependencies, diagrams, analysis, docs, security, debt, and disk space
- **Daemon** — Optional gRPC daemon for persistent management
- **Security audit** — Dependency vulnerabilities, hardcoded secrets, insecure configs, code vulnerability patterns (SQL injection, command injection, path traversal, XSS, SSRF, and more)
- **Docker Intelligence** — Parse Dockerfiles and docker-compose.yml, auto-generate Dockerfiles per framework (Python, Node, Rust, Go, Flutter), generate docker-compose.yml with auto-detected infrastructure (PostgreSQL, Redis, MongoDB, etc.)
- **Security** — Never reads `.env` values; centralized sensitive file protection

## CLI

| Command | Description |
|---------|-------------|
| `void add <name> <path>` | Register project (auto-detects services) |
| `void add-service <project> <name> <cmd> -d <dir>` | Add service manually |
| `void remove <name>` | Unregister project |
| `void list` | List projects and services |
| `void scan <path>` | Preview detection without registering |
| `void start <project> [-s service]` | Start all or one service |
| `void stop <project> [-s service]` | Stop all or one service |
| `void status <project>` | Live status: PIDs, URLs, uptime |
| `void check <project>` | Verify dependencies |
| `void diagram <project> [-f mermaid\|drawio]` | Generate diagrams |
| `void audit <project> [-o file]` | Security audit |
| `void analyze <project> [--compare] [--cross-project] [--best-practices]` | Code analysis |
| `void docker <project> [--generate-dockerfile] [--generate-compose] [--save]` | Docker intelligence |

**Flags:** `--wsl` (WSL paths), `--daemon` (connect to daemon), `--compare` (compare snapshots), `--cross-project` (inter-project deps), `--label <tag>` (tag snapshot)

## TUI Dashboard

```bash
void-tui                # All projects
void-tui my-project     # Specific project
void-tui --daemon       # Via daemon
```

| Key | Action |
|-----|--------|
| `a` | Start all services |
| `s` | Start selected service |
| `k` | Stop selected service |
| `K` | Stop all |
| `j`/`↓` | Navigate down |
| `l` | Toggle log panel |
| `Tab` | Switch panel |
| `r` | Refresh status |
| `?` | Help |
| `q` | Quit (stops services) |

## Desktop (Tauri)

Desktop app with dark GUI:

- **Services**: Cards with status (running/stopped/failed), PID, uptime, URL, start/stop controls
- **Logs**: Live log viewer with service selector and auto-scroll
- **Dependencies**: Check table with status, version, fix suggestions
- **Diagrams**: Mermaid diagram rendering for architecture, API routes, DB models
- **Analysis**: Architecture patterns, anti-patterns, cyclomatic complexity, coverage visualization
- **Docs**: Render project README and documentation files with markdown styling
- **Disk Space**: Scan project + global caches, view sizes, delete to free space
- **Security**: Risk score, vulnerability findings, secrets detection, config audit
- **Technical Debt**: Metric snapshots with trend comparison, expandable details (god classes, complex functions, anti-patterns, circular deps)
- **Docker**: Parse and analyze existing Docker artifacts, generate Dockerfiles and docker-compose.yml, save to project
- **Sidebar**: Project navigation, add/remove projects, WSL distro browser
- **UX**: Copy buttons on results, educational tooltips, diagram zoom controls, Material Design 3 typography

## MCP Server (AI Integration)

Lets Claude Desktop or Claude Code manage your projects directly.

Add to `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "void-stack": {
      "command": "C:\\path\\to\\void-mcp.exe"
    }
  }
}
```

**Available tools:** `list_projects`, `project_status`, `start_project`, `stop_project`, `start_service`, `stop_service`, `get_logs`, `add_project`, `remove_project`, `check_dependencies`, `read_project_docs`, `read_all_docs`, `generate_diagram`, `analyze_project`, `audit_project`, `scan_directory`, `add_service`, `save_debt_snapshot`, `list_debt_snapshots`, `compare_debt`, `analyze_cross_project`, `scan_project_space`, `scan_global_space`, `docker_analyze`, `docker_generate`

## Dependency Detection

| Detector | Checks |
|----------|--------|
| Python | Binary, version, venv, `pip check` |
| Node | Binary, `node_modules` freshness |
| CUDA | `nvidia-smi`, driver, GPU, VRAM, PyTorch |
| Ollama | Binary, API health, downloaded models |
| Docker | Binary, daemon status, compose |
| Rust | `rustc` and `cargo` versions |
| Go | `go version`, `go.mod` presence |
| Flutter | `flutter --version`, `dart --version`, `pubspec.yaml` |
| .env | Compares `.env` vs `.env.example` |

## Code Analysis

```bash
void analyze my-app -o analysis.md
void analyze my-app --compare --label v2.0
void analyze my-app --cross-project
void analyze my-app --best-practices
void analyze my-app --bp-only         # Only linters, skip architecture
```

- **Patterns** — MVC, Layered, Clean/Hexagonal with confidence
- **Anti-patterns** — God Class, Circular Dependencies, Fat Controllers, Excessive Coupling
- **Cyclomatic complexity** — Per-function (Python, JS/TS, Go, Dart, Rust)
- **Coverage** — LCOV, Cobertura, Istanbul, Go cover profiles
- **Graphs** — Mermaid diagrams of module relationships
- **Trends** — Historical snapshots with comparison

## Diagrams

```bash
void diagram my-app                 # Draw.io (default)
void diagram my-app -f mermaid      # Mermaid markdown
```

Detects: service architecture, external services (by URL extraction from source code and .env), internal service-to-service calls (localhost cross-referencing by port), API routes with Swagger/OpenAPI enrichment (FastAPI, Flask, Express, gRPC/Protobuf), internal vs public API separation, DB models (SQLAlchemy, Django, Prisma, Sequelize, GORM, Drift, Protobuf messages), Rust crate relationships.

## Architecture

```
void-stack/
├── crates/
│   ├── void-stack-core/       # Core library: models, config, runners, detectors, analyzers
│   ├── void-stack-proto/      # Protobuf definitions + gRPC client
│   ├── void-stack-daemon/     # Daemon with gRPC server (tonic)
│   ├── void-stack-tui/        # Terminal dashboard (ratatui)
│   ├── void-stack-mcp/        # MCP server for AI assistants
│   ├── void-stack-desktop/    # Tauri v2 app (React + TypeScript)
│   └── void-stack-cli/        # CLI interface (clap)
├── example-void-stack.toml
└── CHANGELOG.md
```

## Configuration

### `void-stack.toml` (per project)

```toml
name = "my-fullstack-app"
description = "Full stack app"
project_type = "node"

[hooks]
install_deps = true

[[services]]
name = "backend-api"
command = "npm run dev"
target = "wsl"
working_dir = "./backend"

[[services]]
name = "web-frontend"
command = "npm run dev"
target = "windows"
working_dir = "./frontend"

[[services]]
name = "database"
command = "docker compose up postgres redis"
target = "docker"
```

### Global config

All projects are stored in `%LOCALAPPDATA%\void-stack\config.toml`. Each service has an absolute `working_dir`, supporting monorepos and distributed layouts.

## Security

- `.env` is read for **variable names only** — values are never stored or displayed
- Sensitive files (`.env`, `credentials.json`, private keys, `secrets.*`) blocked from analysis and MCP
- Centralized deny-list in `security.rs` covers all file-reading paths

## License

[Business Source License 1.1](LICENSE)

Free for personal and educational use. Commercial use requires a license for organizations with more than 5 employees or more than $100,000 USD in annual revenue. Converts to Apache 2.0 on 2029-03-09.
