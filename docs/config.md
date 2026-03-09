# Void Stack — Config Reference

## `void-stack.toml`

Place this file in your project root. Void Stack will look for it automatically.

### Minimal example

```toml
name = "my-app"
path = "."

[[services]]
name = "server"
command = "npm run dev"
target = "windows"
```

### Full example

```toml
name = "my-fullstack-app"
description = "Full stack app with multiple services"
path = "."
project_type = "node"    # python | node | rust | go | docker | unknown
tags = ["web", "api"]

[hooks]
venv = true              # Create Python venv if missing (Python only)
install_deps = true      # Run pip install / npm install / cargo build
build = false            # Run build step before launching
custom = [               # Custom commands to run before launch
  "echo 'pre-launch hook'"
]

[[services]]
name = "backend-api"
command = "npm run dev"
target = "wsl"           # windows | wsl | docker | ssh
working_dir = "./backend"
enabled = true
env_vars = [
  ["PORT", "3000"],
  ["NODE_ENV", "development"]
]
depends_on = ["database"]

[[services]]
name = "frontend"
command = "npm run dev"
target = "windows"
working_dir = "./frontend"

[[services]]
name = "database"
command = "docker compose up postgres"
target = "docker"
```

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Project name |
| `description` | string | no | Short description |
| `path` | string | yes | Project root path |
| `project_type` | string | no | Auto-detected if omitted |
| `tags` | string[] | no | Tags for organization |
| `hooks` | table | no | Pre-launch hooks |
| `services` | array | yes | List of services |

### Service fields

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | yes | — | Unique service name |
| `command` | string | yes | — | Shell command to run |
| `target` | string | yes | — | Where to run: windows, wsl, docker, ssh |
| `working_dir` | string | no | project path | Override working directory |
| `enabled` | bool | no | true | Whether to include in "start all" |
| `env_vars` | [string, string][] | no | [] | Environment variables |
| `depends_on` | string[] | no | [] | Services that must start first |

### Hook fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `venv` | bool | false | Create Python venv if missing |
| `install_deps` | bool | false | Install dependencies (pip/npm/cargo/go) |
| `build` | bool | false | Run build step |
| `custom` | string[] | [] | Custom shell commands |
