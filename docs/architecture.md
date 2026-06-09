# Void Stack — Architecture

## Workspace Layout

```
void-stack-rs/
├── Cargo.toml                 # Workspace root
├── CHANGELOG.md
├── docs/
│   ├── architecture.md        # This file
│   └── config.md              # Config format reference
├── crates/
│   ├── void-stack-core/        # Library crate (no binary)
│   │   ├── backend.rs         # ServiceBackend trait (direct/daemon abstraction)
│   │   ├── config.rs          # TOML config load/save + project detection
│   │   ├── error.rs           # Error types
│   │   ├── hooks.rs           # Pre-launch hooks (venv, deps, build)
│   │   ├── manager.rs         # ProcessManager (orchestration) + ServiceBackend impl
│   │   ├── model.rs           # Domain entities
│   │   └── runner/
│   │       ├── mod.rs          # Runner trait
│   │       └── local.rs        # Windows + WSL runner
│   ├── void-stack-proto/       # Protobuf + gRPC definitions
│   │   ├── build.rs           # tonic-build protobuf compilation
│   │   ├── proto/
│   │   │   └── void-stack.proto # Service + message definitions
│   │   └── src/
│   │       ├── lib.rs          # Generated code re-export + type conversions
│   │       └── client.rs       # DaemonClient (ServiceBackend over gRPC)
│   ├── void-stack-daemon/      # Daemon binary (gRPC server)
│   │   └── src/
│   │       ├── main.rs         # Entry point, CLI, signal handling
│   │       ├── server.rs       # tonic service implementation
│   │       └── lifecycle.rs    # PID file management
│   ├── void-stack-cli/         # CLI binary
│   │   └── main.rs
│   └── void-stack-tui/         # TUI binary
│       ├── app.rs             # App state + logic
│       ├── main.rs            # Entry point + event loop
│       └── ui.rs              # Ratatui rendering
└── example-void-stack.toml
```

## Dependency Flow

```
void-stack-cli ────┐
                  ├──▶ void-stack-proto ──▶ void-stack-core
void-stack-tui ────┘         ▲
                            │
void-stack-daemon ───────────┘
```

- `core` is a library with zero UI or network dependencies
- `proto` defines the gRPC interface and provides type conversions + DaemonClient
- `daemon` is the background server exposing core via gRPC
- `cli` and `tui` can work in direct mode (core) or daemon mode (proto client)

## ServiceBackend Trait

The key abstraction enabling dual mode (direct vs daemon):

```rust
#[async_trait]
pub trait ServiceBackend: Send + Sync {
    async fn start_all(&self) -> Result<Vec<ServiceState>>;
    async fn start_one(&self, name: &str) -> Result<ServiceState>;
    async fn stop_all(&self) -> Result<()>;
    async fn stop_one(&self, name: &str) -> Result<()>;
    async fn get_states(&self) -> Result<Vec<ServiceState>>;
    async fn get_state(&self, name: &str) -> Result<Option<ServiceState>>;
    async fn refresh_status(&self) -> Result<()>;
}
```

**Implementations:**
- `ProcessManager` — Direct process management (Phase 1)
- `DaemonClient` — gRPC proxy to a running daemon (Phase 2)

## Runner Architecture

```
Runner (trait)
├── LocalRunner        ← Windows (cmd /c) + WSL (wsl -e bash)
├── DockerRunner       ← Future: docker compose / docker run
├── SshRunner          ← Future: remote execution via SSH
└── CloudRunner        ← Future: Vercel/DigitalOcean/AWS APIs
```

Each runner implements:
- `start(service, project_path) -> ServiceState`
- `stop(service, pid)`
- `is_running(pid) -> bool`

## ProcessManager

Central orchestrator that:
1. Receives a `Project` with its services
2. Runs pre-launch hooks (venv, deps install, build)
3. Starts each service using the appropriate runner
4. Tracks PIDs and status in a `HashMap<String, ServiceState>`
5. Periodically refreshes status by checking PIDs
6. Stops all on shutdown

## Daemon Architecture

```
┌──────────┐   ┌──────────┐   ┌──────────┐
│   CLI    │   │   TUI    │   │   MCP    │
└────┬─────┘   └────┬─────┘   └────┬─────┘
     │              │              │
     └──────────────┼──────────────┘
                    │ gRPC (port 50051)
            ┌───────▼────────┐
            │  Daemon Server │ ← void-stack-daemon
            │  (tonic gRPC)  │
            └───────┬────────┘
                    │
            ┌───────▼────────┐
            │ ProcessManager │ ← void-stack-core
            └────────────────┘
```

**gRPC Services (void-stack.proto):**
- `StartAll`, `StartOne` — Service lifecycle
- `StopAll`, `StopOne` — Service termination
- `GetStates`, `GetState` — Status queries
- `RefreshStatus` — Force PID re-check
- `StreamLogs` — Server-side streaming of service output
- `Ping` — Health check with version/uptime info
- `Shutdown` — Graceful daemon termination

**Daemon lifecycle:**
- PID file stored in `%LOCALAPPDATA%\void-stack\daemon.pid`
- Contains PID, port, project path, start time
- Ctrl+C triggers graceful shutdown (stop all services, remove PID file)
- CLI can send `Shutdown` RPC or fallback to `taskkill`

## Mode Selection

CLI/TUI frontends support dual mode:
- **Direct mode** (default): Frontend creates `ProcessManager` directly
- **Daemon mode** (`--daemon`): Frontend connects to running daemon via gRPC

This enables multiple frontends to see the same live state and share process management.


## HNSW rebuild behavior

The semantic index is incremental at the chunk/embedding level: re-indexing only re-embeds files whose content hash changed, and unchanged chunks reuse their cached embeddings from `meta.db`. The HNSW graph, however, is **rebuilt in full on every index run** — the underlying `hnsw_rs` crate has no delete operation, so removing or updating points in place is not possible.

Practical implications:

- Re-index time on large projects is dominated by the HNSW rebuild, not by embedding (which is cached). Expect the rebuild to stay sub-second up to tens of thousands of chunks and to grow to minutes on very large monorepos (100k+ chunks).
- The rebuild is atomic: the new graph is written to a temporary directory and renamed over the old one, so searches never observe a half-built index.
- If rebuild time becomes a problem, candidate strategies are per-directory index sharding, an index structure with native deletes (IVF), or tombstoning with periodic compaction. None is implemented today.
