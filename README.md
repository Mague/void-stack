# DevLaunch

**Got 10 projects with backends, frontends, workers, and databases — and you can't remember how to start any of them?**

DevLaunch fixes that. One command to start your entire dev stack — backend, frontend, database, workers — on Windows, WSL, or Docker. No memorizing ports, no opening 5 terminals, no reading outdated READMEs.

```bash
devlaunch add my-app F:\projects\my-app    # Auto-detects services
devlaunch start my-app                      # Starts everything
```

That's it. DevLaunch scans your project, detects which frameworks you're using (FastAPI, Vite, Express, Django...), generates the right commands, and runs them. If there's a Python venv, it finds it. If `node_modules` is missing, it tells you.

> Built with Rust — fast, reliable, no runtime.

**[Leer en español](README.es.md)**

<!-- TODO: Add screenshot/GIF of TUI and Desktop here -->
<!-- ![DevLaunch TUI](docs/screenshots/tui-dashboard.png) -->
<!-- ![DevLaunch Desktop](docs/screenshots/desktop-app.png) -->

## Interfaces

DevLaunch has **4 interfaces** — use whichever you prefer:

| Interface | Description |
|-----------|-------------|
| **CLI** (`devlaunch.exe`) | Fast commands from terminal |
| **TUI** (`devlaunch-tui.exe`) | Interactive terminal dashboard with live logs |
| **Desktop** (`devlaunch-desktop.exe`) | Desktop app with GUI (Tauri + React) |
| **MCP Server** (`devlaunch-mcp.exe`) | Integration with Claude Desktop / Claude Code |

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
devlaunch add my-app F:\projects\my-app

# DevLaunch detects:
#   ✓ backend  → uvicorn main:app --host 0.0.0.0 --port 8000
#   ✓ frontend → npm run dev
#   ✓ .venv    → auto-resolves python to virtualenv

# 2. Check dependencies
devlaunch check my-app
#   ✅ Python 3.11 (venv detected)
#   ✅ Node 20.x (node_modules up to date)
#   ✅ .env complete vs .env.example

# 3. Start everything
devlaunch start my-app
#   [backend]  → http://localhost:8000
#   [frontend] → http://localhost:5173

# 4. Or open the interactive dashboard
devlaunch-tui my-app
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
git clone https://github.com/your-user/devlaunch-rs.git
cd devlaunch-rs
cargo build --release

# Binaries in target/release/
#   devlaunch.exe           — CLI
#   devlaunch-tui.exe       — Terminal dashboard
#   devlaunch-desktop.exe   — Desktop app (Tauri)
#   devlaunch-daemon.exe    — gRPC daemon
#   devlaunch-mcp.exe       — MCP server for AI
```

### Desktop (Tauri)

```bash
cd crates/devlaunch-desktop/frontend
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
- **Diagrams** — Generates Mermaid and Draw.io from project structure
- **Code analysis** — Dependency graphs, anti-patterns, cyclomatic complexity, coverage
- **Technical debt** — Metric snapshots with trend comparison
- **AI integration** — MCP server with 15 tools for Claude Desktop / Claude Code
- **Disk space scanner** — Scan and clean project deps (node_modules, venv, target) and global caches (npm, pip, Cargo, Ollama, HuggingFace, LM Studio)
- **Desktop GUI** — Tauri app with dark theme, services, logs, dependencies, diagrams, analysis, docs, and disk space
- **Daemon** — Optional gRPC daemon for persistent management
- **Security** — Never reads `.env` values; centralized sensitive file protection

## CLI

| Command | Description |
|---------|-------------|
| `devlaunch add <name> <path>` | Register project (auto-detects services) |
| `devlaunch add-service <project> <name> <cmd> -d <dir>` | Add service manually |
| `devlaunch remove <name>` | Unregister project |
| `devlaunch list` | List projects and services |
| `devlaunch scan <path>` | Preview detection without registering |
| `devlaunch start <project> [-s service]` | Start all or one service |
| `devlaunch stop <project> [-s service]` | Stop all or one service |
| `devlaunch status <project>` | Live status: PIDs, URLs, uptime |
| `devlaunch check <project>` | Verify dependencies |
| `devlaunch diagram <project> [-f mermaid\|drawio]` | Generate diagrams |
| `devlaunch analyze <project> [--compare] [--cross-project]` | Code analysis |

**Flags:** `--wsl` (WSL paths), `--daemon` (connect to daemon), `--compare` (compare snapshots), `--cross-project` (inter-project deps), `--label <tag>` (tag snapshot)

## TUI Dashboard

```bash
devlaunch-tui                # All projects
devlaunch-tui my-project     # Specific project
devlaunch-tui --daemon       # Via daemon
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
- **Sidebar**: Project navigation, add/remove projects

## MCP Server (AI Integration)

Lets Claude Desktop or Claude Code manage your projects directly.

Add to `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "devlaunch": {
      "command": "C:\\path\\to\\devlaunch-mcp.exe"
    }
  }
}
```

**Available tools:** `list_projects`, `project_status`, `start_project`, `stop_project`, `start_service`, `stop_service`, `get_logs`, `add_project`, `remove_project`, `check_dependencies`, `read_project_docs`, `read_all_docs`, `generate_diagram`, `analyze_project`

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
devlaunch analyze my-app -o analysis.md
devlaunch analyze my-app --compare --label v2.0
devlaunch analyze my-app --cross-project
```

- **Patterns** — MVC, Layered, Clean/Hexagonal with confidence
- **Anti-patterns** — God Class, Circular Dependencies, Fat Controllers, Excessive Coupling
- **Cyclomatic complexity** — Per-function (Python, JS/TS, Go, Dart, Rust)
- **Coverage** — LCOV, Cobertura, Istanbul, Go cover profiles
- **Graphs** — Mermaid diagrams of module relationships
- **Trends** — Historical snapshots with comparison

## Diagrams

```bash
devlaunch diagram my-app                 # Draw.io (default)
devlaunch diagram my-app -f mermaid      # Mermaid markdown
```

Detects: service architecture, external services (PostgreSQL, Redis, Ollama, AI APIs, AWS S3), API routes (FastAPI, Flask, Express), DB models (SQLAlchemy, Django, Prisma, Sequelize, GORM), Rust crate relationships.

## Architecture

```
devlaunch-rs/
├── crates/
│   ├── devlaunch-core/       # Core library: models, config, runners, detectors, analyzers
│   ├── devlaunch-proto/      # Protobuf definitions + gRPC client
│   ├── devlaunch-daemon/     # Daemon with gRPC server (tonic)
│   ├── devlaunch-tui/        # Terminal dashboard (ratatui)
│   ├── devlaunch-mcp/        # MCP server for AI assistants
│   ├── devlaunch-desktop/    # Tauri v2 app (React + TypeScript)
│   └── devlaunch-cli/        # CLI interface (clap)
├── example-devlaunch.toml
└── CHANGELOG.md
```

## Configuration

### `devlaunch.toml` (per project)

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

All projects are stored in `%LOCALAPPDATA%\devlaunch\config.toml`. Each service has an absolute `working_dir`, supporting monorepos and distributed layouts.

## Security

- `.env` is read for **variable names only** — values are never stored or displayed
- Sensitive files (`.env`, `credentials.json`, private keys, `secrets.*`) blocked from analysis and MCP
- Centralized deny-list in `security.rs` covers all file-reading paths

## License

MIT
