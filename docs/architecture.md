# DevLaunch — Architecture

## Workspace Layout

```
devlaunch-rs/
├── Cargo.toml                 # Workspace root
├── CHANGELOG.md
├── docs/
│   ├── architecture.md        # This file
│   └── config.md              # Config format reference
├── crates/
│   ├── devlaunch-core/        # Library crate (no binary)
│   │   ├── config.rs          # TOML config load/save + project detection
│   │   ├── error.rs           # Error types
│   │   ├── hooks.rs           # Pre-launch hooks (venv, deps, build)
│   │   ├── manager.rs         # ProcessManager (orchestration)
│   │   ├── model.rs           # Domain entities
│   │   └── runner/
│   │       ├── mod.rs          # Runner trait
│   │       └── local.rs        # Windows + WSL runner
│   ├── devlaunch-cli/         # CLI binary
│   │   └── main.rs
│   └── devlaunch-tui/         # TUI binary
│       ├── app.rs             # App state + logic
│       ├── main.rs            # Entry point + event loop
│       └── ui.rs              # Ratatui rendering
└── example-devlaunch.toml
```

## Dependency Flow

```
devlaunch-cli ──┐
                ├──▶ devlaunch-core
devlaunch-tui ──┘
```

- `core` is a library with zero UI dependencies
- `cli` and `tui` are thin frontends consuming `core`
- Future: `devlaunch-daemon`, `devlaunch-mcp` will also depend on `core`

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

## Future: Daemon Architecture

```
┌──────────┐   ┌──────────┐   ┌──────────┐
│   CLI    │   │   TUI    │   │   MCP    │
└────┬─────┘   └────┬─────┘   └────┬─────┘
     │              │              │
     └──────────────┼──────────────┘
                    │
            ┌───────▼────────┐
            │  Daemon (gRPC) │
            └───────┬────────┘
                    │
            ┌───────▼────────┐
            │     Core       │
            └────────────────┘
```

When the daemon is implemented, CLI/TUI/MCP become thin gRPC clients.
This enables multiple frontends to see the same live state.
