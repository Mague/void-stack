# Changelog

All notable changes to DevLaunch will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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
