<p align="center">
  <img src="crates/void-stack-desktop/icons/icon.svg" alt="Void Stack" width="120" height="120">
</p>

<h1 align="center">Void Stack</h1>

[![CI](https://github.com/Mague/void-stack/actions/workflows/ci.yml/badge.svg)](https://github.com/Mague/void-stack/actions/workflows/ci.yml)
[![Release](https://github.com/Mague/void-stack/actions/workflows/release.yml/badge.svg)](https://github.com/Mague/void-stack/actions/workflows/release.yml)
[![Version](https://img.shields.io/github/v/release/Mague/void-stack?include_prereleases&label=version)](https://github.com/Mague/void-stack/releases/latest)
[![License](https://img.shields.io/github/license/Mague/void-stack)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-669%20passing-brightgreen)]()
[![Coverage](https://img.shields.io/badge/coverage-80.5%25-brightgreen)]()

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

<div align="center">
  <img src="https://github.com/user-attachments/assets/77be9712-0263-4625-953d-5c6163b4de09" alt="Void Stack Desktop — services running" width="100%"/>
  <br/><br/>
  <img src="https://github.com/user-attachments/assets/817b3b04-9347-4bc0-a374-8708694b37fe" alt="Void Stack TUI — navigating tabs" width="80%"/>
</div>

---

## Dogfooding: Void Stack analyzes itself

Void Stack's own analysis and audit tools are used to maintain the quality of its codebase. Here's what running `void analyze void-stack --compare` and `void audit void-stack` on the project itself revealed — and how we used those findings to improve the code:

### Security audit

```bash
void audit void-stack
# Risk Score: 2/100
# 2 low findings (innerHTML usage — already mitigated with DOMPurify)
```

The initial audit found 6 issues (risk score 25/100), but 4 were false positives — regex patterns and templates in the detection code flagged as "secrets". This led us to add smart false-positive filtering (self-referencing file allowlist, regex metacharacter detection, template line filtering), dropping the false positive rate from 83% to 0%.

### Code analysis

```bash
void analyze void-stack --compare --label v0.17.0
# Pattern: Clean / Hexagonal (85% confidence)
# 115 modules, 20,735 LOC, 30 external deps
# Max complexity: 42 (analyze_best_practices) — now refactored to ~15
# Anti-patterns: 23 → reduced High severity from 7 to 3
```

Findings that drove refactoring:

| Finding | Action taken |
|---------|-------------|
| God Class: `cli/main.rs` (1202 LOC, 25 fn) | Split into 6 command modules (~250 LOC main) |
| God Class: `mcp/server.rs` (1197 LOC, 35 fn) | Split into 10 tool modules (~340 LOC server) |
| God Class: `manager.rs` (30 fn) | Split into 4 submodules (process, state, logs, url) |
| God Class + Fat Controller: `vuln_patterns.rs` (789 LOC) | Split into 5 category modules (injection, xss, network, crypto, config) |
| God Class: `db_models.rs` (1065 LOC) | Split into 7 submodules by DB format (python, sequelize, gorm, drift, proto, prisma) |
| God Class: `generate_dockerfile.rs` (821 LOC) | Split into 6 submodules by language (python, node, rust, go, flutter) |
| God Class: `api_routes.rs` (747 LOC) | Split into 5 submodules by protocol (python, node, grpc, swagger) |
| God Class: `architecture.rs` (788 LOC) | Split into 4 submodules (externals, crates, infra) |
| God Class: `classifier.rs` (759 LOC, 44 fn) | Split into 3 submodules (logic, signals/data tables, tests) |
| Fat Controller: `cli/analysis.rs` (580 LOC) | Split into 4 submodules (analyze, diagram, audit, suggest) |
| CC=42: `analyze_best_practices` | Table-driven linter registry (CC ~15) |
| CC=41: `cmd_analyze` | Extracted 11 helper functions (CC ~10) |

### Technical debt tracking

```bash
void analyze void-stack --compare --label v0.22.0
# Pattern: Clean / Hexagonal (85% confidence)
# Coverage: 80.5% (26268/32609 lines) [lcov]
# Explicit debt: 15 markers (TODO: 8, FIXME: 4, HACK: 2, OPTIMIZE: 1)
# 669 tests passing
```

New in v0.22.0: explicit debt markers (TODO/FIXME/HACK/XXX/OPTIMIZE/BUG/TEMP/WORKAROUND) are now scanned from source comments and shown in CLI output, markdown reports, and the desktop Debt tab. Complex functions (CC≥10) are cross-referenced with coverage data — uncovered critical functions get `[!]` warnings in CLI and 🔴 indicators in markdown.

---

## Interfaces

Void Stack has **4 interfaces** — use whichever you prefer:

| Interface | Description |
|-----------|-------------|
| **CLI** (`void`) | Fast commands from terminal |
| **TUI** (`void-stack-tui`) | Interactive terminal dashboard: services, analysis, security audit, debt, space |
| **Desktop** (`void-stack-desktop`) | Desktop app with GUI (Tauri + React) — Windows (.msi), macOS (.dmg), Linux (.deb) |
| **MCP Server** (`void-stack-mcp`) | Integration with Claude Desktop / Claude Code |

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

### Binaries (recommended)

Download pre-built binaries from the [Releases](https://github.com/mague/void-stack/releases) page — no Rust required.

| Platform | File |
|----------|------|
| Windows  | `.msi` / `.exe` (NSIS) |
| macOS    | `.dmg` |
| Linux    | `.deb` / `.AppImage` |

> **macOS note:** If you get *"cannot be opened because the developer cannot be verified"*, run:
> ```bash
> xattr -cr /Applications/Void\ Stack.app
> ```

### From source (Cargo)

```bash
# Core CLI (the main tool)
cargo install --git https://github.com/mague/void-stack void-stack-cli

# TUI Dashboard
cargo install --git https://github.com/mague/void-stack void-stack-tui

# MCP Server (for AI integration with Claude Desktop / Claude Code)
cargo install --git https://github.com/mague/void-stack void-stack-mcp

# gRPC Daemon (optional, for persistent management)
cargo install --git https://github.com/mague/void-stack void-stack-daemon
```

**Prerequisites:** [Rust](https://rustup.rs) + Protobuf compiler (`winget install Google.Protobuf` on Windows)

### Build from source

```bash
git clone https://github.com/mague/void-stack.git
cd void-stack
cargo build --release

# Binaries in target/release/
#   void              — CLI
#   void-stack-tui    — Terminal dashboard
#   void-stack-daemon — gRPC daemon
#   void-stack-mcp    — MCP server for AI
```

### Desktop app (Tauri)

```bash
cd crates/void-stack-desktop
cargo tauri build
# Generates installer in target/release/bundle/
```

## Excluding files from analysis

Create `.voidignore` in your project root to exclude paths from `void analyze`:

```
# Generated code
internal/pb/
vendor/
**/*.pb.go
**/*.pb.gw.go

# Mocks
**/mocks/
**/*_mock.go
```

Same syntax as `.gitignore` (simplified). Supports prefix paths, `**/` glob suffixes, and directory names.

## Features

- **Multi-service** — Start/stop all services together or individually
- **Cross-platform** — Windows (`cmd`), macOS, WSL (`bash`), Docker containers, SSH (future)
- **Auto-detection** — Scans directories and identifies Python, Node, Rust, Go, Flutter, Docker
- **Smart commands** — Detects FastAPI, Flask, Django, Vite, Next.js, Express, Air (Go hot-reload) and generates the right command
- **Pre-launch hooks** — Automatically creates venvs, installs deps (`pip install`, `npm install`, `go mod download`) per service before starting. Works out of the box with no configuration
- **Dependency checking** — Verifies Python, Node, CUDA, Ollama, Docker, Rust, `.env`
- **Live logs** — Stdout/stderr from all services with automatic URL detection
- **Diagrams** — Generates Mermaid and Draw.io from project structure using unified scanners (architecture, API routes with Swagger/OpenAPI enrichment, internal/external API separation, gRPC/Protobuf services, DB models with FK-proximity layout — Prisma, Sequelize, GORM, Django, SQLAlchemy, Drift)
- **Code analysis** — Dependency graphs, anti-patterns, cyclomatic complexity, coverage
- **Best practices** — Native linters (react-doctor, ruff, clippy, golangci-lint, dart analyze) with unified scoring
- **Technical debt** — Metric snapshots with trend comparison
- **AI integration** — MCP server with 20+ tools for Claude Desktop / Claude Code; AI-powered refactoring suggestions via Ollama (local LLM) with graceful fallback. When a semantic index exists, enriches prompts with actual code snippets from complexity hotspots and god classes
- **Disk space scanner** — Scan and clean project deps (node_modules, venv, target) and global caches (npm, pip, Cargo, Ollama, HuggingFace, LM Studio)
- **Desktop GUI** — Tauri app with cyberpunk mission-control aesthetic, visual hierarchy (KPI cards, glow effects, severity gradients), services, logs, dependencies, diagrams, analysis, docs, security, debt, and disk space
- **Daemon** — Optional gRPC daemon for persistent management
- **Security audit** — Dependency vulnerabilities, hardcoded secrets, insecure configs, code vulnerability patterns (SQL injection, command injection, path traversal, XSS, SSRF, and more) with smart false-positive filtering (skips self-referencing detection patterns, regex definitions, templates, JSX elements, git history refactor commits, and test modules via brace-depth tracking)
- **Docker Runner** — Services with `target = "docker"` run inside Docker containers. Four modes: raw docker commands, image references (`postgres:16` → auto `docker run`), Compose auto-detect, and Dockerfile builds. Compose imports as a single `docker compose up` service that launches all containers together. `docker:` prefix separates Docker services from local ones. Per-service config for ports, volumes, and extra args. Process exit watcher detects failures and updates status automatically
- **Docker Intelligence** — Parse Dockerfiles and docker-compose.yml, auto-generate Dockerfiles per framework (Python, Node, Rust, Go, Flutter), generate docker-compose.yml with auto-detected infrastructure (PostgreSQL, Redis, MongoDB, etc.)
- **Infrastructure Intelligence** — Detect Terraform resources (AWS RDS, ElastiCache, S3, Lambda, SQS, GCP Cloud SQL, Azure PostgreSQL), Kubernetes manifests (Deployments, Services, Ingress, StatefulSets), and Helm charts with dependencies — all integrated into architecture diagrams
- **Security** — Never reads `.env` values; centralized sensitive file protection

## CLI

| Command | Description |
|---------|-------------|
| `void add <n> <path>` | Register project (auto-detects services) |
| `void add-service <project> <n> <cmd> -d <dir>` | Add service manually |
| `void remove <n>` | Unregister project |
| `void list` | List projects and services |
| `void scan <path>` | Preview detection without registering |
| `void start <project> [-s service]` | Start all or one service |
| `void stop <project> [-s service]` | Stop all or one service |
| `void status <project>` | Live status: PIDs, URLs, uptime |
| `void check <project>` | Verify dependencies |
| `void diagram <project> [-f mermaid\|drawio] [--print-content]` | Generate diagrams |
| `void audit <project> [-o file]` | Security audit |
| `void analyze <project> [--compare] [--cross-project] [--best-practices]` | Code analysis |
| `void docker <project> [--generate-dockerfile] [--generate-compose] [--save]` | Docker intelligence |
| `void suggest <project> [--model <m>] [--service <s>] [--raw]` | AI refactoring suggestions (Ollama) |
| `void read-file <project> <path>` | Read any project file (blocks .env, credentials) |
| `void logs <project> <service> [-n lines] [--compact] [--raw]` | Show filtered service logs |
| `void index <project> [--force] [--generate-voidignore]` | Index codebase for semantic search |
| `void search <project> "<query>" [-k top_k]` | Semantic code search |
| `void stats [--project <p>] [--days <d>] [--json]` | Token savings statistics |
| `void claudeignore <project> [--dry-run] [--force]` | Generate `.claudeignore` optimized for tech stack |

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
| `1`-`5` | Switch tab (Services/Analysis/Security/Debt/Space) |
| `R` | Run action (analyze, audit, scan) on current tab |
| `j`/`↓` | Navigate down |
| `l` | Toggle log panel |
| `Tab` | Switch panel |
| `r` | Refresh status |
| `L` | Toggle language (ES/EN) |
| `?` | Help |
| `q` | Quit (stops services) |

**i18n:** Spanish (default) and English. Press `L` to toggle.

**Tabs:** Services (manage/monitor), Analysis (architecture pattern, layers, anti-patterns, complexity + coverage cross-ref), Security (risk score, vulnerability findings), Debt (TODO/FIXME/HACK markers), Space (project + global disk usage)

## Desktop (Tauri)

Desktop app with dark GUI:

- **Services**: Cards with status (running/stopped/failed), PID, uptime, URL (opens in browser), start/stop controls, per-technology icons with brand-colored glow on running services, OS-specific target badges (Windows/macOS/Linux/Docker) with automatic platform detection, two-step service removal
- **Logs**: Live log viewer with service selector and auto-scroll
- **Dependencies**: Check table with status, version, fix suggestions
- **Diagrams**: Mermaid rendering + native Draw.io XML rendering (custom SVG renderer with DOMPurify) for architecture, API routes, DB models
- **Analysis**: Architecture patterns, anti-patterns, cyclomatic complexity, coverage visualization
- **Docs**: Render project README and documentation files with markdown styling
- **Disk Space**: Scan project + global caches, view sizes, delete to free space
- **Security**: Risk score, vulnerability findings, secrets detection, config audit
- **Technical Debt**: Metric snapshots with trend comparison, expandable details (god classes, complex functions, anti-patterns, circular deps)
- **Docker**: Parse and analyze existing Docker artifacts, generate Dockerfiles and docker-compose.yml, save to project, detect Terraform/Kubernetes/Helm infrastructure
- **Sidebar**: Project navigation, add/remove projects, WSL distro browser
- **UX**: Copy buttons on results, educational tooltips, diagram zoom controls, Material Design 3 typography

## MCP Server (AI Integration)

Lets Claude Desktop or Claude Code manage your projects directly.

**Windows** — Add to `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "void-stack": {
      "command": "void-stack-mcp.exe"
    }
  }
}
```

**macOS / Linux** — Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `~/.config/Claude/claude_desktop_config.json` (Linux):

```json
{
  "mcpServers": {
    "void-stack": {
      "command": "void-stack-mcp"
    }
  }
}
```

**Available tools:** `list_projects`, `project_status`, `start_project`, `stop_project`, `start_service`, `stop_service`, `get_logs`, `add_project`, `remove_project`, `check_dependencies`, `read_project_docs`, `read_all_docs`, `generate_diagram`, `analyze_project`, `audit_project`, `scan_directory`, `add_service`, `save_debt_snapshot`, `list_debt_snapshots`, `compare_debt`, `analyze_cross_project`, `scan_project_space`, `scan_global_space`, `docker_analyze`, `docker_generate`, `suggest_refactoring`, `generate_claudeignore`, `get_token_stats`, `index_project_codebase`, `semantic_search`, `get_index_stats`

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

[[services]]
name = "cache"
command = "redis:7-alpine"
target = "docker"
[services.docker]
ports = ["6379:6379"]
```

### Global config

All projects are stored in a platform-specific location:
- **Windows:** `%LOCALAPPDATA%\void-stack\config.toml`
- **macOS:** `~/Library/Application Support/void-stack/config.toml`
- **Linux:** `~/.config/void-stack/config.toml`

Each service has an absolute `working_dir`, supporting monorepos and distributed layouts.

## Security

- `.env` is read for **variable names only** — values are never stored or displayed
- Sensitive files (`.env`, `credentials.json`, private keys, `secrets.*`) blocked from analysis and MCP
- Centralized deny-list in `security.rs` covers all file-reading paths

## License

This project is licensed under the [Apache License 2.0](LICENSE). See the LICENSE file for details.
