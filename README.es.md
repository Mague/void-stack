# DevLaunch

**¿Tenés 10 proyectos con backends, frontends, workers y bases de datos, y no recordás cómo levantar ninguno?**

DevLaunch resuelve eso. Un solo comando para arrancar todo tu stack de desarrollo — backend, frontend, base de datos, workers — en Windows, WSL o Docker. Sin recordar puertos, sin abrir 5 terminales, sin leer READMEs viejos.

```bash
devlaunch add mi-app F:\proyectos\mi-app    # Detecta servicios automáticamente
devlaunch start mi-app                       # Levanta todo
```

Eso es todo. DevLaunch escanea tu proyecto, detecta qué frameworks usás (FastAPI, Vite, Express, Django...), genera los comandos correctos, y los ejecuta. Si hay un venv de Python, lo encuentra. Si falta un `node_modules`, te avisa.

> Built with Rust — rápido, confiable, sin runtime.

<!-- TODO: Agregar screenshot/GIF del TUI y Desktop aquí -->
<!-- ![DevLaunch TUI](docs/screenshots/tui-dashboard.png) -->
<!-- ![DevLaunch Desktop](docs/screenshots/desktop-app.png) -->

## Interfaces

DevLaunch tiene **4 interfaces** — usá la que prefieras:

| Interfaz | Descripción |
|----------|-------------|
| **CLI** (`devlaunch.exe`) | Comandos rápidos desde terminal |
| **TUI** (`devlaunch-tui.exe`) | Dashboard interactivo en terminal con logs en vivo |
| **Desktop** (`devlaunch-desktop.exe`) | App de escritorio con UI gráfica (Tauri + React) |
| **MCP Server** (`devlaunch-mcp.exe`) | Integración con Claude Desktop / Claude Code |

## Ejemplo completo: FastAPI + React en 30 segundos

Supongamos que tenés un proyecto con un backend FastAPI y un frontend React:

```
mi-app/
├── backend/       # FastAPI con venv
│   ├── main.py    # from fastapi import FastAPI
│   └── .venv/
├── frontend/      # React con Vite
│   ├── package.json
│   └── src/
└── .env
```

```bash
# 1. Registrar el proyecto (escanea y detecta los servicios)
devlaunch add mi-app F:\proyectos\mi-app

# DevLaunch detecta:
#   ✓ backend  → uvicorn main:app --host 0.0.0.0 --port 8000
#   ✓ frontend → npm run dev
#   ✓ .venv    → auto-resuelve python al virtualenv

# 2. Verificar dependencias
devlaunch check mi-app
#   ✅ Python 3.11 (venv detectado)
#   ✅ Node 20.x (node_modules actualizado)
#   ✅ .env completo vs .env.example

# 3. Levantar todo
devlaunch start mi-app
#   [backend]  → http://localhost:8000
#   [frontend] → http://localhost:5173

# 4. O abrir el dashboard interactivo
devlaunch-tui mi-app
```

## Instalación

### Prerequisitos

- **Rust** (rustc + cargo). Si no lo tenés:
  ```bash
  # Windows (winget)
  winget install Rustlang.Rust.MSVC

  # O desde https://rustup.rs
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

- **Protobuf compiler** (para el daemon gRPC):
  ```bash
  winget install Google.Protobuf
  ```

### Compilar

```bash
git clone https://github.com/your-user/devlaunch-rs.git
cd devlaunch-rs
cargo build --release

# Binarios en target/release/
#   devlaunch.exe           — CLI
#   devlaunch-tui.exe       — Dashboard en terminal
#   devlaunch-desktop.exe   — App de escritorio (Tauri)
#   devlaunch-daemon.exe    — Daemon gRPC
#   devlaunch-mcp.exe       — MCP server para AI
```

### Desktop (Tauri)

```bash
cd crates/devlaunch-desktop/frontend
npm install
npm run build
cd ..
cargo tauri build
# Genera instalador MSI/NSIS en target/release/bundle/
```

## Features

- **Multi-servicio** — Arrancá/detené todos los servicios juntos o individualmente
- **Cross-platform** — Windows (`cmd`), WSL (`bash`), Docker, SSH (futuro)
- **Auto-detección** — Escanea directorios e identifica Python, Node, Rust, Go, Flutter, Docker
- **Comandos inteligentes** — Detecta FastAPI, Flask, Django, Vite, Next.js, Express y genera el comando correcto
- **Hooks pre-launch** — Crea venvs, instala deps, ejecuta builds automáticamente
- **Chequeo de dependencias** — Verifica Python, Node, CUDA, Ollama, Docker, Rust, `.env`
- **Logs en vivo** — Stdout/stderr de todos los servicios con detección automática de URLs
- **Diagramas** — Genera Mermaid y Draw.io desde la estructura del proyecto
- **Análisis de código** — Grafos de dependencias, anti-patrones, complejidad ciclomática, cobertura
- **Deuda técnica** — Snapshots de métricas con comparación de tendencias
- **AI integration** — MCP server con 15 tools para Claude Desktop / Claude Code
- **Escáner de espacio** — Escanea y limpia deps del proyecto (node_modules, venv, target) y cachés globales (npm, pip, Cargo, Ollama, HuggingFace, LM Studio)
- **Desktop GUI** — App Tauri con dark theme, servicios, logs, dependencias, diagramas, análisis, docs y espacio en disco
- **Daemon** — gRPC daemon opcional para gestión persistente
- **Auditoría de seguridad** — Vulnerabilidades en deps (npm/pip/cargo/go), secrets hardcodeados, configs inseguras (CORS, debug, Docker)
- **Seguridad** — Nunca lee valores de `.env`; protección centralizada de archivos sensibles

## CLI

| Comando | Descripción |
|---------|-------------|
| `devlaunch add <name> <path>` | Registrar proyecto (auto-detecta servicios) |
| `devlaunch add-service <project> <name> <cmd> -d <dir>` | Agregar servicio manualmente |
| `devlaunch remove <name>` | Desregistrar proyecto |
| `devlaunch list` | Listar proyectos y servicios |
| `devlaunch scan <path>` | Vista previa de detección sin registrar |
| `devlaunch start <project> [-s service]` | Iniciar todo o un servicio |
| `devlaunch stop <project> [-s service]` | Detener todo o un servicio |
| `devlaunch status <project>` | Estado en vivo: PIDs, URLs, uptime |
| `devlaunch check <project>` | Verificar dependencias |
| `devlaunch diagram <project> [-f mermaid\|drawio]` | Generar diagramas |
| `devlaunch audit <project> [-o file]` | Auditoría de seguridad |
| `devlaunch analyze <project> [--compare] [--cross-project]` | Análisis de código |

**Flags:** `--wsl` (rutas WSL), `--daemon` (conectar al daemon), `--compare` (comparar snapshots), `--cross-project` (dependencias entre proyectos), `--label <tag>` (etiquetar snapshot)

## TUI Dashboard

```bash
devlaunch-tui                # Todos los proyectos
devlaunch-tui my-project     # Proyecto específico
devlaunch-tui --daemon       # Vía daemon
```

| Tecla | Acción |
|-------|--------|
| `a` | Iniciar todos los servicios |
| `s` | Iniciar servicio seleccionado |
| `k` | Detener servicio seleccionado |
| `K` | Detener todos |
| `j`/`↓` | Navegar abajo |
| `l` | Toggle panel de logs |
| `Tab` | Cambiar panel |
| `r` | Refrescar estado |
| `?` | Ayuda |
| `q` | Salir (detiene servicios) |

## Desktop (Tauri)

App de escritorio con interfaz gráfica oscura:

- **Servicios**: Cards con estado (running/stopped/failed), PID, uptime, URL, controles start/stop
- **Registros**: Visor de logs en vivo con selector de servicio y auto-scroll
- **Dependencias**: Tabla de checks con estado, versión, sugerencia de fix
- **Diagramas**: Rendering de diagramas Mermaid para arquitectura, rutas API, modelos DB
- **Análisis**: Patrones de arquitectura, anti-patrones, complejidad ciclomática, visualización de cobertura
- **Docs**: Renderiza README y archivos de documentación con estilo markdown
- **Espacio**: Escanea cachés del proyecto + globales, muestra tamaños, permite eliminar para liberar espacio
- **Seguridad**: Risk score, hallazgos de vulnerabilidad, detección de secrets, auditoría de configs
- **Sidebar**: Navegación entre proyectos, agregar/eliminar proyectos

## MCP Server (AI Integration)

Permite que Claude Desktop o Claude Code gestionen tus proyectos directamente.

Agregar a `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "devlaunch": {
      "command": "C:\\path\\to\\devlaunch-mcp.exe"
    }
  }
}
```

**Tools disponibles:** `list_projects`, `project_status`, `start_project`, `stop_project`, `start_service`, `stop_service`, `get_logs`, `add_project`, `remove_project`, `check_dependencies`, `read_project_docs`, `read_all_docs`, `generate_diagram`, `analyze_project`, `audit_project`

## Detección de dependencias

| Detector | Verifica |
|----------|----------|
| Python | Binario, versión, venv, `pip check` |
| Node | Binario, frescura de `node_modules` |
| CUDA | `nvidia-smi`, driver, GPU, VRAM, PyTorch |
| Ollama | Binario, API health, modelos descargados |
| Docker | Binario, estado del daemon, compose |
| Rust | Versiones de `rustc` y `cargo` |
| Go | `go version`, presencia de `go.mod` |
| Flutter | `flutter --version`, `dart --version`, `pubspec.yaml` |
| .env | Compara `.env` vs `.env.example` |

## Análisis de código

```bash
devlaunch analyze my-app -o analysis.md
devlaunch analyze my-app --compare --label v2.0
devlaunch analyze my-app --cross-project
```

- **Patrones** — MVC, Layered, Clean/Hexagonal con confianza
- **Anti-patrones** — God Class, Dependencias Circulares, Fat Controllers, Acoplamiento Excesivo
- **Complejidad ciclomática** — Por función (Python, JS/TS, Go, Dart, Rust)
- **Cobertura** — LCOV, Cobertura, Istanbul, Go cover profiles
- **Grafos** — Diagramas Mermaid de relaciones entre módulos
- **Tendencias** — Snapshots históricos con comparación

## Diagramas

```bash
devlaunch diagram my-app                 # Draw.io (default)
devlaunch diagram my-app -f mermaid      # Mermaid markdown
```

Detecta: arquitectura de servicios, servicios externos (PostgreSQL, Redis, Ollama, AI APIs, AWS S3), rutas API (FastAPI, Flask, Express), modelos DB (SQLAlchemy, Django, Prisma, Sequelize, GORM), relaciones entre crates Rust.

## Arquitectura

```
devlaunch-rs/
├── crates/
│   ├── devlaunch-core/       # Librería core: modelos, config, runners, detectors, analyzers
│   ├── devlaunch-proto/      # Definiciones Protobuf + cliente gRPC
│   ├── devlaunch-daemon/     # Daemon con servidor gRPC (tonic)
│   ├── devlaunch-tui/        # Dashboard terminal (ratatui)
│   ├── devlaunch-mcp/        # MCP server para AI assistants
│   ├── devlaunch-desktop/    # App Tauri v2 (React + TypeScript)
│   └── devlaunch-cli/        # Interfaz CLI (clap)
├── example-devlaunch.toml
└── CHANGELOG.md
```

## Configuración

### `devlaunch.toml` (por proyecto)

```toml
name = "mi-fullstack-app"
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

### Config global

Todos los proyectos se almacenan en `%LOCALAPPDATA%\devlaunch\config.toml`. Cada servicio tiene `working_dir` absoluto, soportando monorepos y layouts distribuidos.

## Seguridad

- `.env` se lee solo por **nombres de variables** — los valores nunca se almacenan ni muestran
- Archivos sensibles (`.env`, `credentials.json`, claves privadas, `secrets.*`) bloqueados del análisis y MCP
- Deny-list centralizada en `security.rs` cubre todos los paths de lectura

## License

MIT
