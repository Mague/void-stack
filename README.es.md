# Void Stack

**¿Tenés 10 proyectos con backends, frontends, workers y bases de datos, y no recordás cómo levantar ninguno?**

Void Stack resuelve eso. Un solo comando para arrancar todo tu stack de desarrollo — backend, frontend, base de datos, workers — en Windows, WSL o Docker. Sin recordar puertos, sin abrir 5 terminales, sin leer READMEs viejos.

```bash
void add mi-app F:\proyectos\mi-app    # Detecta servicios automáticamente
void start mi-app                       # Levanta todo
```

Eso es todo. Void Stack escanea tu proyecto, detecta qué frameworks usás (FastAPI, Vite, Express, Django...), genera los comandos correctos, y los ejecuta. Si hay un venv de Python, lo encuentra. Si falta un `node_modules`, te avisa.

> **Alto Rendimiento** — Built with Rust. Cero overhead de runtime, inicio instantáneo, mínimo consumo de memoria.

> **Flujo Agéntico** — MCP server con 20+ herramientas permite que Claude Desktop / Claude Code gestione tus servicios, analice código y audite seguridad de forma autónoma.

> **Cloud-Native Roadmap** — Deploy a Vercel, DigitalOcean y más desde la misma config (próximamente).

**[Read in English](README.md)** | **[void-stack.dev](https://void-stack.dev)**

<!-- TODO: Agregar screenshot/GIF del TUI y Desktop aquí -->

## Interfaces

Void Stack tiene **4 interfaces** — usá la que prefieras:

| Interfaz | Descripción |
|----------|-------------|
| **CLI** (`void.exe`) | Comandos rápidos desde terminal |
| **TUI** (`void-tui.exe`) | Dashboard interactivo en terminal con logs en vivo |
| **Desktop** (`void-desktop.exe`) | App de escritorio con UI gráfica (Tauri + React) |
| **MCP Server** (`void-mcp.exe`) | Integración con Claude Desktop / Claude Code |

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
void add mi-app F:\proyectos\mi-app

# Void Stack detecta:
#   ✓ backend  → uvicorn main:app --host 0.0.0.0 --port 8000
#   ✓ frontend → npm run dev
#   ✓ .venv    → auto-resuelve python al virtualenv

# 2. Verificar dependencias
void check mi-app
#   ✅ Python 3.11 (venv detectado)
#   ✅ Node 20.x (node_modules actualizado)
#   ✅ .env completo vs .env.example

# 3. Levantar todo
void start mi-app
#   [backend]  → http://localhost:8000
#   [frontend] → http://localhost:5173

# 4. O abrir el dashboard interactivo
void-tui mi-app
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
git clone https://github.com/mague/void-stack.git
cd void-stack
cargo build --release

# Binarios en target/release/
#   void.exe           — CLI
#   void-tui.exe       — Dashboard en terminal
#   void-desktop.exe   — App de escritorio (Tauri)
#   void-daemon.exe    — Daemon gRPC
#   void-mcp.exe       — MCP server para AI
```

### Desktop (Tauri)

```bash
cd crates/void-stack-desktop/frontend
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
- **Diagramas** — Genera Mermaid y Draw.io desde la estructura del proyecto (arquitectura, rutas API con enriquecimiento Swagger/OpenAPI, separación API interna/externa, servicios gRPC, modelos DB con layout por proximidad FK)
- **Análisis de código** — Grafos de dependencias, anti-patrones, complejidad ciclomática, cobertura
- **Best practices** — Linters nativos (react-doctor, ruff, clippy, golangci-lint, dart analyze) con scoring unificado
- **Deuda técnica** — Snapshots de métricas con comparación de tendencias
- **AI integration** — MCP server con 20+ herramientas para Claude Desktop / Claude Code
- **Escáner de espacio** — Escanea y limpia deps del proyecto (node_modules, venv, target) y cachés globales (npm, pip, Cargo, Ollama, HuggingFace, LM Studio)
- **Desktop GUI** — App Tauri con estética cyberpunk mission-control, jerarquía visual (KPI cards, efectos glow, gradientes por severidad), servicios, logs, dependencias, diagramas, análisis, docs, seguridad, deuda técnica y espacio en disco
- **Daemon** — gRPC daemon opcional para gestión persistente
- **Auditoría de seguridad** — Vulnerabilidades en deps, secrets hardcodeados, configs inseguras, patrones de vulnerabilidad en código (inyección SQL, XSS, SSRF, y más)
- **Seguridad** — Nunca lee valores de `.env`; protección centralizada de archivos sensibles

## CLI

| Comando | Descripción |
|---------|-------------|
| `void add <name> <path>` | Registrar proyecto (auto-detecta servicios) |
| `void add-service <project> <name> <cmd> -d <dir>` | Agregar servicio manualmente |
| `void remove <name>` | Desregistrar proyecto |
| `void list` | Listar proyectos y servicios |
| `void scan <path>` | Vista previa de detección sin registrar |
| `void start <project> [-s service]` | Iniciar todo o un servicio |
| `void stop <project> [-s service]` | Detener todo o un servicio |
| `void status <project>` | Estado en vivo: PIDs, URLs, uptime |
| `void check <project>` | Verificar dependencias |
| `void diagram <project> [-f mermaid\|drawio]` | Generar diagramas |
| `void audit <project> [-o file]` | Auditoría de seguridad |
| `void analyze <project> [--compare] [--cross-project] [--best-practices]` | Análisis de código |

**Flags:** `--wsl` (rutas WSL), `--daemon` (conectar al daemon), `--compare` (comparar snapshots), `--cross-project` (dependencias entre proyectos), `--label <tag>` (etiquetar snapshot)

## TUI Dashboard

```bash
void-tui                # Todos los proyectos
void-tui my-project     # Proyecto específico
void-tui --daemon       # Vía daemon
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
- **Deuda Técnica**: Snapshots de métricas con comparación de tendencias, detalles expandibles (god classes, funciones complejas, anti-patrones, deps circulares)
- **Sidebar**: Navegación entre proyectos, agregar/eliminar proyectos, explorador de archivos WSL
- **UX**: Botones de copiar en resultados, tooltips educativos, zoom en diagramas, tipografía Material Design 3

## MCP Server (AI Integration)

Permite que Claude Desktop o Claude Code gestionen tus proyectos directamente.

Agregar a `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "void-stack": {
      "command": "C:\\path\\to\\void-mcp.exe"
    }
  }
}
```

**Tools disponibles:** `list_projects`, `project_status`, `start_project`, `stop_project`, `start_service`, `stop_service`, `get_logs`, `add_project`, `remove_project`, `check_dependencies`, `read_project_docs`, `read_all_docs`, `generate_diagram`, `analyze_project`, `audit_project`, `scan_directory`, `add_service`, `save_debt_snapshot`, `list_debt_snapshots`, `compare_debt`, `analyze_cross_project`, `scan_project_space`, `scan_global_space`

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
void analyze my-app -o analysis.md
void analyze my-app --compare --label v2.0
void analyze my-app --cross-project
void analyze my-app --best-practices
void analyze my-app --bp-only         # Solo linters, omite análisis de arquitectura
```

- **Patrones** — MVC, Layered, Clean/Hexagonal con confianza
- **Anti-patrones** — God Class, Dependencias Circulares, Fat Controllers, Acoplamiento Excesivo
- **Complejidad ciclomática** — Por función (Python, JS/TS, Go, Dart, Rust)
- **Cobertura** — LCOV, Cobertura, Istanbul, Go cover profiles
- **Grafos** — Diagramas Mermaid de relaciones entre módulos
- **Tendencias** — Snapshots históricos con comparación

## Diagramas

```bash
void diagram my-app                 # Draw.io (default)
void diagram my-app -f mermaid      # Mermaid markdown
```

Detecta: arquitectura de servicios, servicios externos (por extracción de URLs del código fuente y .env), llamadas internas entre servicios (cruce de localhost por puerto), rutas API con enriquecimiento Swagger/OpenAPI (FastAPI, Flask, Express, gRPC/Protobuf), separación de APIs internas vs públicas, modelos DB (SQLAlchemy, Django, Prisma, Sequelize, GORM, Drift, mensajes Protobuf), relaciones entre crates Rust.

## Arquitectura

```
void-stack/
├── crates/
│   ├── void-stack-core/       # Librería core: modelos, config, runners, detectors, analyzers
│   ├── void-stack-proto/      # Definiciones Protobuf + cliente gRPC
│   ├── void-stack-daemon/     # Daemon con servidor gRPC (tonic)
│   ├── void-stack-tui/        # Dashboard terminal (ratatui)
│   ├── void-stack-mcp/        # MCP server para AI assistants
│   ├── void-stack-desktop/    # App Tauri v2 (React + TypeScript)
│   └── void-stack-cli/        # Interfaz CLI (clap)
├── example-void-stack.toml
└── CHANGELOG.md
```

## Configuración

### `void-stack.toml` (por proyecto)

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

Todos los proyectos se almacenan en `%LOCALAPPDATA%\void-stack\config.toml`. Cada servicio tiene `working_dir` absoluto, soportando monorepos y layouts distribuidos.

## Seguridad

- `.env` se lee solo por **nombres de variables** — los valores nunca se almacenan ni muestran
- Archivos sensibles (`.env`, `credentials.json`, claves privadas, `secrets.*`) bloqueados del análisis y MCP
- Deny-list centralizada en `security.rs` cubre todos los paths de lectura

## License

[Business Source License 1.1](LICENSE)

Libre para uso personal y educativo. Uso comercial requiere licencia para organizaciones con más de 5 empleados o más de $100,000 USD en ingresos anuales. Se convierte en Apache 2.0 el 2029-03-09.
