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

<div align="center">
  <img src="https://github.com/user-attachments/assets/77be9712-0263-4625-953d-5c6163b4de09" alt="Void Stack Desktop — services running" width="100%"/>
  <br/><br/>
  <img src="https://github.com/user-attachments/assets/817b3b04-9347-4bc0-a374-8708694b37fe" alt="Void Stack TUI — navigating tabs" width="80%"/>
</div>

## Interfaces

Void Stack tiene **4 interfaces** — usá la que prefieras:

| Interfaz | Descripción |
|----------|-------------|
| **CLI** (`void`) | Comandos rápidos desde terminal |
| **TUI** (`void-stack-tui`) | Dashboard interactivo: servicios, análisis, auditoría de seguridad, deuda, espacio |
| **Desktop** (`void-stack-desktop`) | App de escritorio con UI gráfica (Tauri + React) — Windows (.msi), macOS (.dmg), Linux (.deb) |
| **MCP Server** (`void-stack-mcp`) | Integración con Claude Desktop / Claude Code |

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

Void Stack es un ecosistema unificado con múltiples componentes. Podés instalarlos individualmente con Cargo:

### Desde GitHub (recomendado)

```bash
# CLI principal (la herramienta core)
cargo install --git https://github.com/mague/void-stack void-stack-cli

# TUI Dashboard
cargo install --git https://github.com/mague/void-stack void-stack-tui

# MCP Server (para integración con Claude Desktop / Claude Code)
cargo install --git https://github.com/mague/void-stack void-stack-mcp

# Daemon gRPC (opcional, para gestión persistente)
cargo install --git https://github.com/mague/void-stack void-stack-daemon
```

> **Nota:** Binarios pre-compilados para Windows, macOS y Linux estarán disponibles próximamente en la página de [Releases](https://github.com/mague/void-stack/releases).

### Prerequisitos (para compilar desde código fuente)

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

### Compilar desde código fuente

```bash
git clone https://github.com/mague/void-stack.git
cd void-stack
cargo build --release

# Binarios en target/release/
#   void           — CLI
#   void-stack-tui — Dashboard en terminal
#   void-stack-daemon — Daemon gRPC
#   void-stack-mcp — MCP server para AI
```

### App de escritorio (Tauri)

La app de escritorio requiere un proceso de compilación separado (o descarga el instalador desde [Releases](https://github.com/mague/void-stack/releases)):

```bash
cd crates/void-stack-desktop
cargo tauri build
# Genera instalador en target/release/bundle/
#   Windows: .msi / .exe (NSIS)
#   macOS:   .dmg
#   Linux:   .deb / .AppImage
```

> **Nota macOS:** Si aparece *"no se puede abrir porque no se puede verificar el desarrollador"*, ejecuta:
> ```bash
> xattr -cr /Applications/Void\ Stack.app
> ```
> Esto es necesario porque la app aún no está firmada con un certificado de Apple Developer.

## Excluir archivos del análisis

Crea `.voidignore` en la raíz de tu proyecto para excluir paths de `void analyze`:

```
# Código generado
internal/pb/
vendor/
**/*.pb.go
**/*.pb.gw.go

# Mocks
**/mocks/
**/*_mock.go
```

Misma sintaxis que `.gitignore` (simplificada). Soporta prefijos de paths, globs `**/` y nombres de directorio.

## Features

- **Multi-servicio** — Arrancá/detené todos los servicios juntos o individualmente
- **Cross-platform** — Windows (`cmd`), macOS, WSL (`bash`), contenedores Docker, SSH (futuro)
- **Auto-detección** — Escanea directorios e identifica Python, Node, Rust, Go, Flutter, Docker
- **Comandos inteligentes** — Detecta FastAPI, Flask, Django, Vite, Next.js, Express y genera el comando correcto
- **Hooks pre-launch** — Crea venvs, instala deps, ejecuta builds automáticamente
- **Chequeo de dependencias** — Verifica Python, Node, CUDA, Ollama, Docker, Rust, `.env`
- **Logs en vivo** — Stdout/stderr de todos los servicios con detección automática de URLs
- **Diagramas** — Genera Mermaid y Draw.io desde la estructura del proyecto usando scanners unificados (arquitectura, rutas API con enriquecimiento Swagger/OpenAPI, separación API interna/externa, servicios gRPC/Protobuf, modelos DB con layout por proximidad FK — Prisma, Sequelize, GORM, Django, SQLAlchemy, Drift)
- **Análisis de código** — Grafos de dependencias, anti-patrones, complejidad ciclomática, cobertura
- **Best practices** — Linters nativos (react-doctor, ruff, clippy, golangci-lint, dart analyze) con scoring unificado
- **Deuda técnica** — Snapshots de métricas con comparación de tendencias
- **AI integration** — MCP server con 20+ herramientas para Claude Desktop / Claude Code; sugerencias de refactorización con IA via Ollama (LLM local) con fallback elegante
- **Escáner de espacio** — Escanea y limpia deps del proyecto (node_modules, venv, target) y cachés globales (npm, pip, Cargo, Ollama, HuggingFace, LM Studio)
- **Desktop GUI** — App Tauri con estética cyberpunk mission-control, jerarquía visual (KPI cards, efectos glow, gradientes por severidad), servicios, logs, dependencias, diagramas, análisis, docs, seguridad, deuda técnica y espacio en disco
- **Daemon** — gRPC daemon opcional para gestión persistente
- **Auditoría de seguridad** — Vulnerabilidades en deps, secrets hardcodeados, configs inseguras, patrones de vulnerabilidad en código (inyección SQL, XSS, SSRF, y más) con filtrado inteligente de falsos positivos (omite patrones de detección auto-referenciales, definiciones regex, templates, elementos JSX y commits de refactor en historial git)
- **Docker Runner** — Servicios con `target = "docker"` se ejecutan dentro de contenedores Docker. Cuatro modos: comandos docker crudos, referencias a imagen (`postgres:16` → auto `docker run`), auto-detección de Compose, y builds desde Dockerfile. Compose se importa como un solo servicio `docker compose up` que levanta todos los containers juntos. Prefijo `docker:` separa servicios Docker de los locales. Config por servicio para puertos, volúmenes y args extra. Watcher de procesos detecta fallos y actualiza el estado automáticamente
- **Docker Intelligence** — Parsea Dockerfiles y docker-compose.yml, auto-genera Dockerfiles por framework (Python, Node, Rust, Go, Flutter), genera docker-compose.yml con infraestructura auto-detectada (PostgreSQL, Redis, MongoDB, etc.)
- **Infrastructure Intelligence** — Detecta recursos Terraform (AWS RDS, ElastiCache, S3, Lambda, SQS, GCP Cloud SQL, Azure PostgreSQL), manifiestos Kubernetes (Deployments, Services, Ingress, StatefulSets) y charts Helm con dependencias — todo integrado en diagramas de arquitectura
- **Seguridad** — Nunca lee valores de `.env`; protección centralizada de archivos sensibles

## CLI

| Comando | Descripción |
|---------|-------------|
| `void add <n> <path>` | Registrar proyecto (auto-detecta servicios) |
| `void add-service <project> <n> <cmd> -d <dir>` | Agregar servicio manualmente |
| `void remove <n>` | Desregistrar proyecto |
| `void list` | Listar proyectos y servicios |
| `void scan <path>` | Vista previa de detección sin registrar |
| `void start <project> [-s service]` | Iniciar todo o un servicio |
| `void stop <project> [-s service]` | Detener todo o un servicio |
| `void status <project>` | Estado en vivo: PIDs, URLs, uptime |
| `void check <project>` | Verificar dependencias |
| `void diagram <project> [-f mermaid\|drawio] [--print-content]` | Generar diagramas |
| `void audit <project> [-o file]` | Auditoría de seguridad |
| `void analyze <project> [--compare] [--cross-project] [--best-practices]` | Análisis de código |
| `void docker <project> [--generate-dockerfile] [--generate-compose] [--save]` | Docker intelligence |
| `void suggest <project> [--model <m>] [--service <s>] [--raw]` | Sugerencias AI de refactorización (Ollama) |
| `void read-file <project> <path>` | Leer cualquier archivo del proyecto (bloquea .env, credenciales) |

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
| `1`-`5` | Cambiar tab (Services/Analysis/Security/Debt/Space) |
| `R` | Ejecutar acción (analizar, auditar, escanear) en tab actual |
| `j`/`↓` | Navegar abajo |
| `l` | Toggle panel de logs |
| `Tab` | Cambiar panel |
| `r` | Refrescar estado |
| `L` | Cambiar idioma (ES/EN) |
| `?` | Ayuda |
| `q` | Salir (detiene servicios) |

**i18n:** Español (por defecto) e Inglés. Presiona `L` para cambiar.

**Tabs:** Servicios (gestionar/monitorear), Análisis (patrón de arquitectura, capas, anti-patrones, complejidad + cobertura), Seguridad (risk score, hallazgos), Deuda (marcadores TODO/FIXME/HACK), Espacio (uso de disco proyecto + global)

## Desktop (Tauri)

App de escritorio con interfaz gráfica oscura:

- **Servicios**: Cards con estado (running/stopped/failed), PID, uptime, URL (abre en navegador), controles start/stop, iconos por tecnología con glow en color de marca al estar corriendo, badges de target por SO (Windows/macOS/Linux/Docker) con detección automática de plataforma, eliminación de servicios con confirmación
- **Registros**: Visor de logs en vivo con selector de servicio y auto-scroll
- **Dependencias**: Tabla de checks con estado, versión, sugerencia de fix
- **Diagramas**: Rendering Mermaid + rendering nativo de Draw.io XML (renderizador SVG custom con DOMPurify) para arquitectura, rutas API, modelos DB
- **Análisis**: Patrones de arquitectura, anti-patrones, complejidad ciclomática, visualización de cobertura
- **Docs**: Renderiza README y archivos de documentación con estilo markdown
- **Espacio**: Escanea cachés del proyecto + globales, muestra tamaños, permite eliminar para liberar espacio
- **Seguridad**: Risk score, hallazgos de vulnerabilidad, detección de secrets, auditoría de configs
- **Deuda Técnica**: Snapshots de métricas con comparación de tendencias, detalles expandibles (god classes, funciones complejas, anti-patrones, deps circulares)
- **Docker**: Parsea y analiza artefactos Docker existentes, genera Dockerfiles y docker-compose.yml, guarda en proyecto
- **Sidebar**: Navegación entre proyectos, agregar/eliminar proyectos, explorador de archivos WSL
- **UX**: Botones de copiar en resultados, tooltips educativos, zoom en diagramas, tipografía Material Design 3

## MCP Server (AI Integration)

Permite que Claude Desktop o Claude Code gestionen tus proyectos directamente.

**Windows** — Agregar a `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "void-stack": {
      "command": "void-stack-mcp.exe"
    }
  }
}
```

**macOS / Linux** — Agregar a `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) o `~/.config/Claude/claude_desktop_config.json` (Linux):

```json
{
  "mcpServers": {
    "void-stack": {
      "command": "void-stack-mcp"
    }
  }
}
```

**Tools disponibles:** `list_projects`, `project_status`, `start_project`, `stop_project`, `start_service`, `stop_service`, `get_logs`, `add_project`, `remove_project`, `check_dependencies`, `read_project_docs`, `read_all_docs`, `generate_diagram`, `analyze_project`, `audit_project`, `scan_directory`, `add_service`, `save_debt_snapshot`, `list_debt_snapshots`, `compare_debt`, `analyze_cross_project`, `scan_project_space`, `scan_global_space`, `docker_analyze`, `docker_generate`, `suggest_refactoring`

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

[[services]]
name = "cache"
command = "redis:7-alpine"
target = "docker"
[services.docker]
ports = ["6379:6379"]
```

### Config global

Todos los proyectos se almacenan en una ubicación específica de la plataforma:
- **Windows:** `%LOCALAPPDATA%\void-stack\config.toml`
- **macOS:** `~/Library/Application Support/void-stack/config.toml`
- **Linux:** `~/.config/void-stack/config.toml`

Cada servicio tiene `working_dir` absoluto, soportando monorepos y layouts distribuidos.

## Dogfooding: Void Stack se analiza a sí mismo

Las herramientas de análisis y auditoría de Void Stack se usan para mantener la calidad de su propio código. Esto es lo que encontró `void analyze devlaunch-rs --compare` y `void audit devlaunch-rs` — y cómo usamos esos hallazgos para mejorar:

### Auditoría de seguridad

```bash
void audit devlaunch-rs
# Risk Score: 2/100
# 2 hallazgos low (uso de innerHTML — ya mitigado con DOMPurify)
```

La auditoría inicial encontró 6 issues (risk score 25/100), pero 4 eran falsos positivos — patrones regex y templates en el código de detección marcados como "secrets". Esto nos llevó a agregar filtrado inteligente (allowlist de archivos auto-referenciales, detección de metacaracteres regex, filtrado de templates), bajando los falsos positivos de 83% a 0%.

### Análisis de código

```bash
void analyze devlaunch-rs --compare --label v0.17.0
# Patrón: Clean / Hexagonal (85% confianza)
# 115 módulos, 20,735 LOC, 30 deps externas
# Complejidad máx: 42 (analyze_best_practices) — refactorizado a ~15
# Anti-patrones: 23 → severidad High reducida de 7 a 3
```

Hallazgos que motivaron refactorizaciones:

| Hallazgo | Acción tomada |
|----------|--------------|
| God Class: `cli/main.rs` (1202 LOC, 25 fn) | Dividido en 6 módulos de comandos (~250 LOC main) |
| God Class: `mcp/server.rs` (1197 LOC, 35 fn) | Dividido en 10 módulos de tools (~340 LOC server) |
| God Class: `manager.rs` (30 fn) | Dividido en 4 submódulos (process, state, logs, url) |
| God Class + Fat Controller: `vuln_patterns.rs` (789 LOC) | Dividido en 5 módulos por categoría (injection, xss, network, crypto, config) |
| God Class: `db_models.rs` (1065 LOC) | Dividido en 7 submódulos por formato DB (python, sequelize, gorm, drift, proto, prisma) |
| God Class: `generate_dockerfile.rs` (821 LOC) | Dividido en 6 submódulos por lenguaje (python, node, rust, go, flutter) |
| God Class: `api_routes.rs` (747 LOC) | Dividido en 5 submódulos por protocolo (python, node, grpc, swagger) |
| God Class: `architecture.rs` (788 LOC) | Dividido en 4 submódulos (externals, crates, infra) |
| God Class: `classifier.rs` (759 LOC, 44 fn) | Dividido en 3 submódulos (lógica, tablas de señales, tests) |
| Fat Controller: `cli/analysis.rs` (580 LOC) | Dividido en 4 submódulos (analyze, diagram, audit, suggest) |
| CC=42: `analyze_best_practices` | Registro de linters table-driven (CC ~15) |
| CC=41: `cmd_analyze` | Extraídas 11 funciones helper (CC ~10) |

### Tracking de deuda técnica

```bash
void analyze devlaunch-rs --compare --label v0.22.0
# Patrón: Clean / Hexagonal (85% confianza)
# Cobertura: 80.5% (26268/32609 líneas) [lcov]
# Deuda explícita: 15 marcadores (TODO: 8, FIXME: 4, HACK: 2, OPTIMIZE: 1)
# 669 tests pasando
```

Nuevo en v0.22.0: los marcadores de deuda explícita (TODO/FIXME/HACK/XXX/OPTIMIZE/BUG/TEMP/WORKAROUND) ahora se escanean de los comentarios del código y se muestran en la salida CLI, reportes markdown y la pestaña Deuda del desktop. Las funciones complejas (CC≥10) se cruzan con datos de cobertura — las funciones críticas sin cobertura reciben advertencias `[!]` en CLI e indicadores 🔴 en markdown.

El `Excessive Coupling` en `lib.rs` (16 módulos) es esperado para el entry point de un crate. `drawio.rs` se redujo de ~1100 LOC a ~550 LOC eliminando scanners duplicados (ahora compartidos con Mermaid vía `scan_raw`).

## Seguridad

- `.env` se lee solo por **nombres de variables** — los valores nunca se almacenan ni muestran
- Archivos sensibles (`.env`, `credentials.json`, claves privadas, `secrets.*`) bloqueados del análisis y MCP
- Deny-list centralizada en `security.rs` cubre todos los paths de lectura

## License

Este proyecto está licenciado bajo la [Apache License 2.0](LICENSE). Consulta el archivo LICENSE para más detalles.