# Changelog

All notable changes to DevLaunch will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.7.0] - 2026-03-08

### Added

#### Code Analysis (`devlaunch-core/analyzer`) — Phase 7b
- **Dependency graph builder**: static import analysis for Python and JS/TS
  - Python: `import`, `from ... import`, relative imports
  - JS/TS: ES modules (`import ... from`), CommonJS (`require`), re-exports
  - Auto-detects project language from manifest files or source files
- **Layer classification**: Controller, Service, Repository, Model, Utility, Config, Test
  - Based on directory names, file names, and content heuristics (route decorators, DB patterns)
- **Architecture pattern detection**: MVC, Layered, Clean/Hexagonal, Monolith
  - Confidence scoring based on layer presence and dependency flow
- **Anti-pattern detection**:
  - **God Class**: files >500 LOC or >15 functions
  - **Circular Dependency**: Tarjan's SCC algorithm
  - **Fat Controller**: controllers >200 LOC or importing repositories directly
  - **No Service Layer**: controllers without intermediate service layer
  - **Excessive Coupling**: modules with fan-out >10
- **Markdown documentation**: architecture summary, layer distribution, dependency map (Mermaid), module table, coupling metrics, anti-pattern report with fix suggestions

#### Draw.io Diagram Support
- **`.drawio` format**: multi-page XML files for diagrams.net / VS Code Draw.io extension
- Architecture + API Routes as separate pages
- Default format changed from Mermaid to draw.io
- Diagrams saved to project directory by default

#### CLI
- **`devlaunch analyze <project> [-o file] [-s service]`**: full code analysis with pattern/anti-pattern detection
- **`devlaunch diagram <project> [-f drawio|mermaid] [-o file]`**: format selection flag

#### MCP Server
- **`analyze_project`** tool: returns markdown analysis with architecture patterns and anti-patterns
- **`generate_diagram`** tool: now supports `format` parameter ("mermaid" or "drawio")

## [0.6.0] - 2026-03-08

### Added

#### Mermaid Diagram Generation (`devlaunch-core`) — Phase 7
- **Architecture diagrams** (`graph TB`): auto-detects service types (Frontend/Backend/Worker), ports, connections (frontend→backend), and external services (PostgreSQL, Redis, Ollama, AI APIs, AWS S3)
- **API route diagrams** (`graph LR`): scans FastAPI/Flask decorators (`@app.get`, `@router.post`) and Express routes (`app.get`, `router.post`) with method-colored badges
- **DB model diagrams** (`erDiagram`): detects SQLAlchemy (Column + Mapped), Django (models.Model), and Prisma schema models with field types

#### CLI
- **`devlaunch diagram <project> [-o file]`**: generate all diagrams to stdout or file

#### MCP Server
- **`generate_diagram`** tool: returns Mermaid markdown for architecture, API routes, and DB models

## [0.5.0] - 2026-03-08

### Added

#### Dependency Detection (`devlaunch-core`) — Phase 4
- **7 dependency detectors** running in parallel with 3s per-command timeout and 10s global timeout:
  - **PythonDetector**: python/python3/py binary, version, venv detection (searches up to 4 ancestor dirs), `pip check` for broken packages
  - **NodeDetector**: node/npm version, `node_modules/` existence, staleness check vs `package.json` modified time
  - **CudaDetector**: `nvidia-smi` (driver version, GPU name, VRAM), CUDA version, PyTorch `torch.cuda.is_available()` check
  - **OllamaDetector**: binary version, API health (`/api/tags`), lists downloaded models
  - **DockerDetector**: binary version, daemon status (`docker info`), docker compose availability
  - **RustDetector**: `rustc` and `cargo` versions
  - **EnvDetector**: compares `.env` vs `.env.example`/`.env.sample`, lists missing variables
- **`DependencyDetector` trait**: `is_relevant()` (auto-skip irrelevant checks) + `check()` async
- **`check_project()`**: scans all service directories, deduplicates by dep type
- **Actionable fix hints**: every failing check includes a copy-pasteable command to fix it

#### CLI
- **`devlaunch check <project>`**: verify all dependencies before starting services
  - Scans project root + all service working directories
  - Shows ✅ OK, ❌ MISSING, ⚠️ NOT RUNNING, 🔧 NEEDS SETUP, ❓ UNKNOWN
  - Summary: "3/4 dependencies ready"

#### MCP Server
- **`check_dependencies` tool**: check all deps for a project via AI assistant
- **`read_project_docs` tool**: read README.md, CHANGELOG.md, CLAUDE.md from project dirs
  - Security: only allows markdown/text/config file extensions
  - Lists available doc files if requested file not found
  - Truncates files > 50KB

---

## [0.4.0] - 2026-03-08

### Added

#### MCP Server (`devlaunch-mcp`) — Phase 3
- **MCP protocol server** using the official Rust SDK (`rmcp`) for AI assistant integration
- **stdio transport**: Communicates via stdin/stdout JSON-RPC (compatible with Claude Code, Cursor, etc.)
- **8 tools exposed**:
  - `list_projects` — List all registered projects with their services
  - `project_status` — Get live status of all services (running, stopped, PIDs, URLs)
  - `start_project` — Start all services in a project
  - `stop_project` — Stop all services in a project
  - `start_service` — Start a specific service within a project
  - `stop_service` — Stop a specific service within a project
  - `get_logs` — Get recent log output from a service (configurable line count)
  - `add_project` — Scan a directory and register it as a project with auto-detected services
  - `remove_project` — Unregister a project (stops running services first)
- **Process managers** created on-demand per project, reused across tool calls
- **JSON responses** with structured data for all status/list operations

#### Smart Command Detection (`devlaunch-core`)
- **Python framework auto-detection**: Analyzes source files to determine the correct start command
  - **FastAPI/Starlette**: `uvicorn module:app --host 0.0.0.0 --port 8000`
  - **Flask**: `flask --app varname run --port 5000`
  - **Django**: `python manage.py runserver`
  - **Self-starting** (has `uvicorn.run()` in `__main__`): `python filename.py`
  - **Generic `__main__`**: `python filename.py`
- **App variable detection**: Finds the ASGI/WSGI variable name (e.g., `server = FastAPI()` → `server`)
- **Candidate file scanning**: Checks `main.py`, `app.py`, `server.py`, `run.py`, `manage.py` in order
- **Per-project-type defaults**: Node → `npm run dev`, Rust → `cargo run`, Go → `go run .`, Docker → `docker compose up`

#### TUI (`devlaunch-tui`)
- **Multi-project dashboard**: Shows all registered projects from global config
- **Three-panel layout**: Projects list (left), Services table (right), Logs (bottom)
- **Tab/Shift+Tab** to cycle between panels, arrow keys to navigate
- **Project indicator**: ● green (has running services) / ○ gray (all stopped)
- **Optional filter**: `devlaunch-tui [project_name]` for single-project view
- Stops all services across ALL projects on quit

---

## [0.3.0] - 2026-03-08

### Added

#### Core (`devlaunch-core`)
- **Log capture**: Piped stdout/stderr from child processes via `BufReader::lines()` with background tokio tasks
- **URL auto-detection**: Regex-based detection of `http://localhost:PORT` URLs from service output, stored in `ServiceState.url`
- **ANSI stripping**: Strip terminal escape codes before URL matching (Vite, Next.js colorize URLs)
- **Python virtualenv auto-detection**: Automatically resolves `python` commands to `venv/Scripts/python.exe` (searches working_dir and parent for monorepos)
- **`get_logs()` method**: Retrieve captured log lines per service (up to 5000 lines buffered)

#### CLI (`devlaunch-cli`)
- **URL display**: Detected service URLs shown in real-time as services start
- **Continuous polling**: Uses `tokio::select!` to poll for URLs indefinitely while waiting for Ctrl+C (no timeout)

#### TUI (`devlaunch-tui`)
- **URL column**: Service table now shows detected URLs in blue

### Fixed

#### Windows child process stability
- **stdin closed** (`Stdio::null()`): Prevents Node.js deadlock when child processes try to read inherited stdin ([nodejs/node#56537](https://github.com/nodejs/node/issues/56537))
- **`FORCE_COLOR=1`**: Forces Vite to output server URLs even when stdout is piped/non-TTY ([vitejs/vite#11262](https://github.com/vitejs/vite/issues/11262))
- **`PYTHONUNBUFFERED=1`**: Ensures Python output arrives in real-time instead of being buffered
- **`cmd /c call`**: Keeps pipes alive for batch files (.cmd) that cmd.exe would otherwise replace
- **`\\?\` path stripping**: Removes extended-length prefix from `canonicalize()` paths that break Node.js/Python
- **Removed `kill_on_drop(true)`**: `TerminateProcess` only kills cmd.exe, not child process trees; `taskkill /T /F` handles cleanup correctly

### Changed
- URL polling in CLI changed from fixed 30-second window to continuous polling with `tokio::select!`
- Log readers now use `Arc<Mutex<>>` shared state for thread-safe access from background tasks

#### Testing
- 27 unit tests (added: URL detection, ANSI stripping, venv resolution, Python framework detection, app variable detection)

---

## [0.2.0] - 2026-03-08

### Added

#### Proto (`devlaunch-proto`)
- **Protobuf service definition**: `devlaunch.proto` with 10 RPCs (StartAll, StartOne, StopAll, StopOne, GetStates, GetState, RefreshStatus, StreamLogs, Shutdown, Ping)
- **Type conversions**: Bidirectional `From` impls between core types (`ServiceState`, `ServiceStatus`) and protobuf types
- **DaemonClient**: gRPC client implementing `ServiceBackend` trait for transparent daemon mode

#### Daemon (`devlaunch-daemon`)
- **gRPC server**: Tonic-based server exposing all ProcessManager operations
- **PID file management**: Write/read/cleanup daemon info in `%LOCALAPPDATA%\devlaunch\`
- **Graceful shutdown**: Ctrl+C handler stops all services and removes PID file
- **Daemon subcommands**: `start` (with `--port`), `stop` (graceful via gRPC or fallback kill), `status` (live ping)

#### Core (`devlaunch-core`)
- **`ServiceBackend` trait**: Async abstraction over service management, implemented by both `ProcessManager` (direct) and `DaemonClient` (gRPC)

#### CLI (`devlaunch-cli`)
- **Dual mode**: `--daemon` flag to connect via gRPC instead of managing processes directly
- **`daemon start|stop|status`** subcommands for daemon lifecycle management
- **`--port`** flag for custom daemon port

#### TUI (`devlaunch-tui`)
- **Dual mode**: `--daemon` flag to connect via gRPC to a running daemon
- Refactored to use `ServiceBackend` trait instead of direct `ProcessManager`
- In daemon mode, does not stop services on quit (daemon manages lifecycle)

#### Global Config & Project Management
- **Centralized config** in `%LOCALAPPDATA%\devlaunch\config.toml` — manage all projects from one place
- **`devlaunch add <name> <path>`** — Register a project with auto-detected services (monorepo aware)
- **`devlaunch add-service`** — Add individual services with custom paths to any project
- **`devlaunch remove`** — Unregister a project
- **`devlaunch list`** — Show all registered projects and their services
- **`devlaunch scan`** — Preview what devlaunch detects in a directory
- **`--wsl` flag** — Scan and add projects inside WSL with Linux paths
- **Monorepo support** — Scans subdirectories (2 levels deep) for project markers
- **Distributed projects** — Each service has its own absolute `working_dir`, enabling cross-folder grouping
- **WSL scanning** — Single optimized `find` command via WSL for fast detection

### Changed
- CLI and TUI `start`/`stop` commands now use `ServiceBackend` trait for pluggable backends
- CLI redesigned around project-centric workflow (start/stop take project name, not path)

## [0.1.0] - 2026-03-08

### Added

#### Core (`devlaunch-core`)
- **Project model**: `Project`, `Service`, `Target` (Windows/WSL/Docker/SSH), `ServiceState`, `ServiceStatus`
- **TOML config**: Load/save `devlaunch.toml` project files
- **Project detection**: Auto-detect Python, Node, Rust, Go, Docker projects by file markers
- **Local runner**: Execute processes on Windows (`cmd /c`) and WSL (`wsl -e bash -c`)
- **Process manager**: Start/stop all or individual services, track PIDs, refresh status
- **Pre-launch hooks**: Auto-create Python venv, install dependencies (pip/npm/cargo/go), run builds, custom commands
- **Error handling**: Typed errors with `thiserror`

#### CLI (`devlaunch-cli`)
- `devlaunch start [service]` — Start all or a specific service
- `devlaunch stop [service]` — Stop all or a specific service
- `devlaunch status` — Show project info and service list
- `devlaunch init` — Generate `devlaunch.toml` with auto-detected project type
- `devlaunch detect` — Show detected project type

#### TUI (`devlaunch-tui`)
- Real-time service dashboard with Ratatui
- Service table: name, target, status (color-coded), PID, uptime
- Log panel: per-service stdout/stderr output
- Keyboard navigation: start/stop services, view logs, refresh, help overlay
- Auto-refresh status every 1 second
- Graceful shutdown: stops all services on quit

#### Testing
- 7 unit tests: config loading, project type detection, runner start/is_running
