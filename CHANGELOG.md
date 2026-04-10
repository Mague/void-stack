# Changelog

All notable changes to Void Stack will be documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.23.4] - 2026-04-10

### Changed
- **Auto version sync** — Pre-commit hook reads version from CHANGELOG.md and patches Cargo.toml + tauri.conf.json automatically. CI validates version matches tag before building

## [0.23.3] - 2026-04-10

### Changed
- **MCP: vector search always included** — `void-stack-mcp` Cargo.toml now activates `void-stack-core/vector` directly, so `cargo build -p void-stack-mcp` always includes semantic search without `--features vector`. CLI and TUI keep vector as optional for minimal builds
- **CI: release binaries include vector search** — `release.yml` now builds CLI and TUI with `--features vector`, and MCP gets it via Cargo.toml. Per-crate build steps replace the single `--workspace` build for granular feature control
- **macOS MCP setup: absolute path + Gatekeeper fix** — README instructions now use full absolute path (`/Users/YOUR_USERNAME/.cargo/bin/void-stack-mcp`) since Claude Desktop launches with minimal PATH. Added `xattr -d com.apple.quarantine` step to prevent silent blocking

### Added
- **OpenCode MCP setup** — README now includes OpenCode configuration (`opencode.json`) with note about free models (MiniMax, Qwen, DeepSeek) — zero API cost MCP integration

## [0.23.2] - 2026-04-10

### Added
- **God Class thresholds by language** — Dart (800/1500 LOC, 25/40 fn), Rust (600/1200 LOC, 20/35 fn), Go/Python/JS/TS keep defaults (500/1000, 15/25). Reduces false positives for Flutter widgets and Rust modules. 4 new tests
- **Hooks: pure command-building functions** — `build_install_deps_command()` and `build_build_command()` return `(program, args)` without spawning processes, enabling unit testing. `needs_venv()` predicate added. 17 new tests covering all ProjectType variants
- **ProcessManager tests** — 11 new tests for `new()`, `is_service_running`, `collect_running_pids`, `stop_all`/`stop_one`, `start_all`/`start_one` with echo dummy services
- **Detector `is_relevant` tests** — 27 new tests covering all 14 detectors (Python, Node, Rust, Go, Flutter, Docker, Env, CUDA, Clippy, Ruff, GolangciLint) using tempfile marker files
- **`process_util` tests** — 9 new tests for `install_hint` (7 known tools + 1 unknown) and `shell_command_sync`
- **Empty HNSW validation** — `ensure_hnsw_cached` rejects indexes with 0 points, returning an actionable error instead of silently returning 0 search results
- **MCP semantic_search query validation** — Queries < 2 words return a helpful hint instead of empty results

### Changed
- **MCP tool descriptions rewritten** — `get_index_stats` is now the recommended entry point ("START HERE"); `semantic_search` promoted as primary code understanding tool; `read_all_docs` demoted to documentation-only; `read_project_file` marked as fallback. Server instructions updated with index-first workflow
- **Atomic HNSW writes** — Index is written to a temp directory then renamed atomically, preventing concurrent readers from seeing a half-written index during force re-index
- **Stats: filter test artifacts** — `.tmp*` projects (from tempfile::tempdir) excluded from global aggregates. `vector_index` (0% savings infrastructure op) excluded from `avg_savings_pct` but still visible in `by_operation`. 2 new tests
- **TUI `handle_key` CC 58→10** — Extracted `handle_analysis_key`, `navigate_projects` as tab-specific dispatchers. Further split `handle_analysis_key` (CC 32→8) into `action_index_project`, `action_generate_voidignore`, `action_suggest`, `action_start_search`, `handle_search_input`, `action_run_search`

### Fixed
- **Vector index: mutex poison recovery** — `update_job()`/`read_job()` helpers recover from poisoned mutex via `poisoned.into_inner()`, ensuring background indexing always transitions to `Completed`/`Failed`. 2 new tests
- **MCP `get_index_stats`: race condition** — Checks disk when registry says `Running` at 100% files, returning real stats instead of stale "in progress" message
- **Dart parser: false positive function_count** — Excludes `setState()`, `showDialog()`, `Navigator.*`, named callbacks (`onPressed: () {`), and lambda arguments from function count
- **Dart parser: block comments excluded from LOC** — Tracks `/* */` and `/** */` blocks, matching Rust parser behavior. Also excludes `///` doc comments
- **Desktop TS build errors** — Fixed missing `}` in `ServiceDashboard.tsx` JSX block and unused `project` param in `StatsPanel.tsx`

### Refactored
- **`vector_index.rs` (1526 LOC) → 7 submodules** — `mod.rs` (re-exports + tests), `indexer.rs` (background jobs, file collection), `search.rs` (embedding model, HNSW cache), `chunker.rs` (function-aware chunking), `db.rs` (SQLite metadata), `voidignore.rs` (.voidignore generator), `stats.rs` (index stats, paths)
- **`global_config.rs` (707 LOC) → 4 submodules** — `paths.rs` (config dir/path), `project_ops.rs` (find/remove project/service), `scanner.rs` (scan_subprojects, command detection), `mod.rs` (GlobalConfig struct, load/save, re-exports + tests)

## [0.23.1] - 2026-04-09

### Added
- **Vector index: hardcoded baseline exclusions** — The indexer now skips additional build artifact directories (`venv/`, `.next/`, `.nuxt/`, `.dart_tool/`, `.turbo/`, `coverage/`) and generated file patterns (`*.pb.rs`, `*_pb2.py`, `*.pb.go`, `*.g.dart`, `*.freezed.dart`, `*.gen.go`, `lcov.info`, `coverage.xml`) even without ignore files. 2 new tests
- **New: `void index --generate-voidignore`** — Generate a `.voidignore` file optimized for semantic index quality. Unlike `.claudeignore` (token reduction), this focuses on excluding generated code, test fixtures, and files without business-logic semantics. Detects tech stack (Rust, Go, Node, Python, Flutter) for stack-specific patterns. Available in CLI (`void index <project> --generate-voidignore`), MCP tool (`generate_voidignore`), Desktop (Tauri command). 4 new tests
- **Vector index respects `.voidignore` on re-index** — Incremental re-indexing now re-reads `.voidignore` each time, so adding exclusions takes effect on the next `void index` run. 1 new regression test
- **AI suggestions enriched with code context from semantic index** — `suggest_refactoring` now queries the vector index for code snippets of high-complexity functions (CC ≥ 10) and god classes, including up to 3 relevant chunks per hotspot in the LLM prompt. Falls back gracefully when no index exists. All interfaces (CLI, MCP, Desktop) upgraded. 2 new tests

- **OnceLock-cached regexes** — `strip_ansi`, `is_progress_line`, and `detect_url` now compile their Regex patterns once via `OnceLock` instead of per-call `Regex::new`. Deduplicated `strip_ansi` across 3 files into a single `log_filter::strip_ansi` source
- **Modules table: top 30 by LOC** — `write_modules_table` now sorts by LOC descending (most relevant first) and caps at 30 rows with overflow note. Reduces token waste in large projects
- **MCP `analyze_project` compact mode** — New `compact: true` parameter produces ~90% smaller output: one-line summary, High anti-patterns only, top-5 complex functions, coverage. No modules table, Mermaid diagram, or deps list
- **MCP `audit_project` severity-differentiated output** — Default: full detail for Critical/High, title+file for Medium, count for Low/Info. `verbose: true` for full JSON (original behavior)
- **`generate_docs` verbose parameter** — When `verbose=false` (MCP): skip empty coverage hints, limit external deps to 10 if >20, omit Mermaid diagram if >20 nodes, skip empty debt section. MCP callers use `verbose=false` by default. 2 new tests
- **Function-aware chunking for vector index** — `chunk_file` now detects function boundaries (Rust, Go, Python, Dart, JS/TS) via regex + brace depth heuristics. Functions up to 150 lines stay in a single chunk; larger functions split with signature repeated in continuation chunks. Falls back to line-based chunking for unsupported extensions. Fixes incomplete function indexing (e.g., Dart `_handleUnpublish` cut at 40 lines). **Existing indexes need `void index <project> --force` to benefit.** 6 new tests

### Changed
- **Vector index: cached embedding model and HNSW index** — The FastEmbed model (`OnceLock<Mutex<TextEmbedding>>`) is now initialized once and reused across all index/search calls, saving 500ms-2s per call. HNSW indexes are cached per-project in a static `HashMap`, loaded from disk only on first access or after re-indexing. Subsequent `semantic_search` calls take <100ms instead of 1-2s
- **Vector index: truly incremental re-indexing** — Embeddings are now stored in SQLite (`embedding BLOB` column, auto-migrated). On incremental re-index, only modified files get re-embedded; cached embeddings for unchanged files are loaded from SQLite. Reduces re-index time 10-50x for projects with few changes (e.g., 1 file changed out of 246 → only 1 file embedded instead of all 1592 chunks)
- **Parallel `stop_all`** — Kill signals and process verification are now sent in parallel via `tokio::spawn` instead of sequentially. With N=8 services, stop time drops from ~2-4s to ~500ms (one parallel kill round + 300ms global wait + one parallel verify round)
- **SQLite batch queries and transactions** — `load_chunks_with_embeddings` now uses batched `IN (...)` queries (max 900 params per batch) instead of per-file queries. `save_chunk_order` and `save_embeddings` wrapped in explicit transactions, reducing from N implicit transactions to 1
- **Log reader batching** — `process_log_line` replaced with `read_lines_batched` that accumulates up to 64 lines before flushing to shared state. Reduces lock acquisitions from ~1000/s to ~20/s for verbose services (Vite, webpack). Partial batches flush after 50ms timeout to keep UI responsive
- **Exit watcher: `child.wait()` instead of PID polling** — Process exit detection now uses efficient `child.wait()` instead of spawning `tasklist` every 2 seconds per service. Eliminates N×(tasklist process/2s) overhead. The `children` HashMap was removed from `ProcessManager` since child handles are now owned by the exit watcher task
- **CLI: `cmd_audit` and `cmd_analyze` now async with `spawn_blocking`** — Security audit and code analysis (which run external tools like clippy, ruff, cargo audit) no longer block the tokio runtime thread. Matches the pattern already used by the Desktop app

### Fixed
- **Security audit: `scan_rust_unwrap` skips production code after `#[cfg(test)]`** — The scanner used `return` when hitting `#[cfg(test)]` or `#[test]`, abandoning the entire file and missing production `.unwrap()`/`.expect()` calls placed after test modules. Replaced with a brace-depth-tracking flag (`in_test_block`) that only skips lines inside the test module and resumes scanning production code once the module closes. 1 new regression test
- **Security audit: `scan_go_error_discard` tautological heuristic** — The `has_err_context` guard was always true because `trimmed.ends_with(')')` matches any function call, causing false positives. Replaced with a focused heuristic: checks discard patterns (`_ =`, `_, _ :=`) at start of expression, excludes type conversions (`[]byte(...)`, `string(...)`, etc.) and known non-error-returning functions (`fmt.Println`, `len()`, etc.). 3 new tests
- **Daemon: inconsistent "service not found" gRPC responses** — `get_state` handled the missing-service case with ad-hoc `Option::None → Status::not_found`, while `stop_one`/`start_one` used `VoidStackError::ServiceNotFound`. Now `ProcessManager::get_state` returns `Result<ServiceState>` with `ServiceNotFound` error, and all daemon handlers use a shared `to_grpc_status` helper for consistent error mapping

## [0.23.0] - 2026-04-07

### Added
- **New: `.claudeignore` generator** — Generate a `.claudeignore` file optimized for your project's tech stack (Rust, Go, Flutter, Node, Python). Reduces Claude Code token consumption by excluding build artifacts, generated files, and dependencies. Detects monorepo/mixed stacks. Available in all 4 interfaces: CLI (`void claudeignore <project>`), TUI (`G` key), MCP tool (`generate_claudeignore`), Desktop (`.claudeignore` button in Services toolbar). 19 unit tests
- **New: vector index & semantic search** — Index project codebase with BAAI/bge-small-en-v1.5 (fastembed, 100% local) + HNSW (hnsw_rs). Respects .claudeignore. Incremental re-indexing. CLI: `void index/search`. TUI: `/` search + `I` index in Analysis tab. MCP: `index_project_codebase`, `semantic_search`, `get_index_stats`. Desktop: search bar + index button in Analysis panel. 9 unit tests
- **New: token savings statistics** — Track how much noise Void Stack filters across all operations. SQLite-backed (`~/.void-stack/stats.db`), auto-records from log_filter and claudeignore. CLI: `void stats [--project] [--days] [--json]`. TUI: tab 6 (Stats). MCP: `get_token_stats` tool. Desktop: Stats tab with bar charts. 9 unit tests
- **New: intelligent log filtering** — RTK-style noise reduction for service logs. Strips ANSI codes, deduplicates consecutive lines (→ `msg (×N)`), removes progress bars/download indicators, compact mode filters INFO/DEBUG keeping only WARN/ERROR, auto-truncates long output. CLI: `void logs <project> <service> [--compact] [--raw]`. TUI: `f` key toggles filter in Logs panel. MCP: `get_logs` auto-filters (pass `raw: true` to disable). Desktop: "Filter noise" toggle in log viewer. 32 unit tests

## [0.22.8] - 2026-03-28

### Added
- **New: Air hot-reload detection for Go** — When scanning Go projects, detects `.air.toml` and uses `air` as the service command instead of `go run .`. Supports multiple Air services within a single Go module (e.g. `cmd/api/.air.toml` and `cmd/worker/.air.toml`). 5 unit tests
- **Auto install dependencies on start** — When no hooks are configured, Void Stack now automatically runs `venv` creation and `pip install`/`npm install`/`go mod download` before starting services. Runs per service working_dir, not just project root
- **Python: broader entrypoint detection** — Projects with `pyproject.toml` or `requirements.txt` (without `main.py`) are now detected as valid Python services. Fixes projects like humbolt_reader where the backend wasn't auto-detected
- **New: `.voidignore` support** — Create a `.voidignore` file in your project root to exclude paths from analysis. Supports prefix paths, glob suffixes (`**/*.pb.go`), directory names. Language-agnostic. 17 unit tests

### Fixed
- **Runner: venv paths with dots fail on Windows** — `cmd /c call .venv\Scripts\python.exe` fails silently because `cmd.exe` misparses paths with dots. Now quotes the executable path when it contains `.venv` or `.env`
- **Logs: strip ANSI escape codes** — Service logs (Vite, npm, etc.) no longer contain raw ANSI color codes (`[32m`, `[39m`). Stripped before storage and display

### Changed
- **Refactor: split `diagram/drawio.rs` (812 LOC, CC=40+22)** — Divided into 4 submodules
- **Refactor: split `diagram/architecture/infra.rs` (CC=35)** — Divided into terraform/kubernetes/helm submodules
- **Refactor: split `analyzer/docs.rs` (708 LOC, CC=44)** — Divided into markdown/coverage/sanitize submodules

## [0.22.7] - 2026-03-18

### Changed
- **MCP: `generate_diagram` returns inline XML** — When format is `drawio`, the tool now saves the file to disk AND returns the full XML content in the response. LLMs can inspect the diagram directly without needing `read_project_file`
- **CLI: `void diagram --print-content`** — New flag to print the full diagram content (drawio XML or mermaid markdown) to stdout after saving
- **MCP: workflow hints in tool descriptions** — All tool descriptions now include usage order guidance so LLMs know the natural flow (`read_all_docs` → `analyze_project` → `generate_diagram` → `audit_project`). Server instructions updated with recommended tool flow

### Fixed
- **Diagrams: duplicate gRPC RPCs in API Routes** — Proto files in `proto/` subdirectory were scanned twice (once via root dir traversal, once via explicit `proto/` entry). Now deduplicates using canonical paths in a HashSet

## [0.22.6] - 2026-03-18

### Added
- **New: `read_project_file` command** — Read any file from a registered project by relative path. Blocks sensitive files (.env, credentials, private keys) via `is_sensitive_file()`. Truncates files >200KB with warning. Available in CLI (`void read-file`), MCP tool, and Desktop (Tauri command)
- **New: `list_project_files` command** — List all files in a project (up to 3 levels deep), excluding sensitive files and build directories (node_modules, target, .git, dist). Available in MCP tool and Desktop
- **Core: `file_reader` module** — New `void-stack-core/src/file_reader.rs` with `read_project_file()` and `list_project_files()` functions, path traversal protection, and canonical path validation. 18 unit tests covering normal reads, sensitive file blocking, traversal attacks, size truncation, and file listing

## [0.22.5] - 2026-03-16

### Fixed
- **Desktop: macOS PATH resolution** — Replaced custom shell-spawn PATH detection with `fix-path-env` crate (official Tauri solution). Called as the first line of `main()` to fix PATH for the entire process, ensuring all child processes (npm, node, cargo, etc.) inherit the full developer PATH when the app is launched from Finder/Launchpad/Dock
- **Desktop: macOS platform detection** — Added `Target::MacOS` variant to the `Target` enum. Services created on macOS now correctly show the Apple logo instead of the Windows logo. New services default to the native platform via `Target::native()` (compile-time detection)
- **Desktop: delete project dialog stuck** — Replaced browser-native `confirm()` (which froze on macOS WebKit) with Tauri's native `tauriConfirm()` dialog. The confirmation modal now closes correctly on both OK and Cancel

### Changed
- **Desktop: platform-aware UI** — On macOS, the sidebar hides Win/WSL target buttons (not applicable). The service dashboard shows macOS/Docker target options instead of Windows/WSL/Docker

## [0.22.4] - 2026-03-15

### Fixed
- **Desktop: macOS PATH detection v2** — Two-layer approach: (1) login shell with `TERM=xterm-256color` forced to trigger NVM/Volta/pyenv initialization, with length validation (>20 chars) to detect incomplete PATH; (2) fallback that manually constructs PATH by checking known developer tool locations (`~/.cargo/bin`, `~/.volta/bin`, `/opt/homebrew/bin`, `~/.nvm/versions/node/*/bin`, `~/.pyenv/shims`, `~/.rbenv/shims`) with `Path::exists()`. Fixes #20

### Added
- **Docs: macOS Gatekeeper note** — README (EN/ES) now includes `xattr -cr` workaround for unsigned app warning

## [0.22.3] - 2026-03-14

### Fixed
- **Desktop: macOS PATH detection** — GUI apps launched from Finder/Dock now resolve the full user PATH from a login shell (zsh → bash → env fallback). Cached with `OnceLock` to avoid repeated shell spawns. Fixes detection of Homebrew, NVM, Volta, Cargo and other developer tools (#20)

## [0.22.2] - 2026-03-13

### Added
- **CI/CD: GitHub Actions workflows** — `ci.yml` runs fmt + clippy + test on push/PR to main/develop. `release.yml` builds cross-platform binaries (Windows x64, macOS x64/ARM, Linux x64) on tag push or manual trigger, and creates GitHub Releases with packaged assets
- **README badges** — CI status, Release status, Version, License, Rust edition, Tests, Coverage
- **Tests: void-stack-core coverage from 72% to 80.5%** — added 534 new unit tests across 16 modules:
  - `ai/mod.rs` — suggestion parsing, priority detection, file path extraction, serde roundtrips
  - `ai/prompt.rs` — prompt builder with coverage, complexity, circular deps, all antipattern kinds
  - `analyzer/docs.rs` — markdown generation, coverage hints per language, sanitize_id
  - `analyzer/imports/mod.rs` — language detection, import resolution, graph building
  - `analyzer/best_practices/mod.rs` — category/severity Display, compute_scores, merge_linter_output, serde
  - `analyzer/best_practices/report.rs` — markdown report, score labels, severity grouping
  - `analyzer/cross_project.rs` — JSON/TOML/setup.py extraction, mermaid links, package scanning
  - `analyzer/history.rs` — create_snapshot with complexity/antipatterns, coverage delta comparison
  - `detector/mod.rs` — DependencyType/CheckStatus Display, constructors, detector count, serde
  - `diagram/architecture/infra.rs` — Terraform/K8s/Helm infrastructure diagrams
  - `diagram/mod.rs` — DiagramFormat, generate_all orchestration
  - `docker/generate_dockerfile/mod.rs` — Dockerfile generation for Python/Node/Rust/Go/Flutter
  - `docker/parse.rs` — Dockerfile/compose parsing, CMD forms, env/volumes/healthcheck/depends_on
  - `global_config.rs` — find/remove project, default commands, entrypoint detection, subproject scanning
  - `runner/docker.rs` — Docker mode detection, compose file finding, image detection
  - `runner/local.rs` — shell escaping, Python venv resolution, UNC path conversion

## [0.22.1] - 2026-03-13

### Fixed
- **Desktop: async analysis command** — `analyze_project_cmd` now runs in `spawn_blocking` instead of blocking the main Tauri thread. Prevents UI freeze during long-running clippy/best-practices analysis on large workspaces
- **TUI: missing i18n keys** — hardcoded "All projects refreshed" and "Status refreshed" now use `t()` with proper Spanish/English translations
- **TUI: format!() with empty args** — removed unnecessary empty string arguments in debt/space status messages
- **TUI: wildcard match on AppTab** — replaced `_` catch-all with explicit variant listing for compile-time safety when adding new tabs
- **TUI: Vec pre-allocation** — added `with_capacity()` for project entries and space scan results

## [0.22.0] - 2026-03-11

### Changed
- **Refactor: split God Class files into submodules** — reduces anti-pattern count and improves maintainability:
  - `classifier.rs` (759 LOC, 44 functions) → `classifier/mod.rs` (logic), `classifier/signals.rs` (data tables), `classifier/tests.rs`
  - `analysis.rs` CLI (580 LOC, 4 commands) → `analysis/mod.rs`, `analysis/analyze.rs`, `analysis/diagram.rs`, `analysis/audit.rs`, `analysis/suggest.rs`
  - `db_models.rs` (1065 LOC) → 7 submodules by DB format (python, sequelize, gorm, drift, proto, prisma)
  - `generate_dockerfile.rs` (821 LOC) → 6 submodules by language (python, node, rust, go, flutter)
  - `api_routes.rs` (747 LOC) → 5 submodules by protocol (python, node, grpc, swagger)
  - `architecture.rs` (788 LOC) → 4 submodules (externals, crates, infra)
- **Coverage: workspace-root search** — `parse_coverage()` now walks parent directories to find workspace-level `lcov.info`/`coverage.xml` for Rust workspace crates. Enables `cargo-llvm-cov` reports to be picked up per-crate
- **Cross-platform coverage** — switched from `cargo-tarpaulin` (Linux-only) to `cargo-llvm-cov` (Windows, macOS, Linux). First coverage report: 42.7% for void-stack-core

### Fixed
- **MCP service name matching** — `analyze_project` and `suggest_refactoring` now match service names by suffix (e.g. `"void-stack-core"` finds `"crates/void-stack-core"`) instead of requiring exact match
- **MCP error message** — "No analyzable code found" now lists all supported languages (Python, JS/TS, Rust, Go, Dart) instead of only Python/JS
- **7 Unknown classifier files** — added `pub(crate) fn` and `pub(super) fn` content signals so Rust utility files with restricted visibility are correctly classified

### Added
- **Explicit debt scanning** — detects TODO, FIXME, HACK, XXX, OPTIMIZE, BUG, TEMP, WORKAROUND markers in source code comments. Language-aware comment detection (20+ file extensions). Integrated into CLI summary, markdown docs, and desktop DTO
- **Coverage cross-reference for critical functions** — complex functions (CC≥10) are cross-referenced against coverage reports. Shows ✅/🔴 indicators in markdown tables. CLI prints uncovered critical functions with `[!]` warnings
- **Unsafe error handling audit** — new security scanner detecting: Rust `.unwrap()`/`.expect()` outside tests, Python bare `except:`/`except Exception: pass`, JS/TS empty catch blocks, Go error discard (`_ =`), Dart bare `catch` without `on`
- **TUI: tab system with 5 tabs** — Services (existing), Analysis (architecture pattern, layers, anti-patterns, complexity with coverage cross-ref), Security (risk score, findings table), Debt (TODO/FIXME/HACK markers), Space (disk usage scanner). Tab switching with 1-5 keys, R to run tab actions
- **TUI: split `ui.rs` into 8 submodules** — header, footer, tabs, services, analysis, security, debt, space, help (prevents God Class as features grow)
- **Void Stack logo** — SVG component in desktop sidebar, Unicode glyphs (⬢◆●) in TUI header
- **TUI: i18n (Spanish/English)** — lightweight match-based i18n with ~170 translated keys covering all UI elements. Press `L` to toggle language. Spanish is the default, matching the desktop app
- **Total tests:** 226 passing (up from 158)

### Fixed
- **TUI: double-key navigation on Windows** — each keypress moved 2 positions because crossterm reports both Press and Release events. Now filters `KeyEventKind::Press` only
- **TUI: double project navigation on Services tab** — global j/k handler and panel handler both fired, moving 2 positions. Separated logic so Services tab delegates entirely to panel handlers
- **TUI: help overlay logo mismatch** — help screen used different glyphs/colors than header logo. Now uses same `/◇\` characters with brand RGB colors

## [0.21.0] - 2026-03-10

### Added
- **Unix/macOS support for LocalRunner:** Services now execute via `sh -c` on Unix instead of `cmd /c` (Windows-only). Python virtualenvs resolve from `bin/` (Unix) or `Scripts/` (Windows) automatically
- **Cross-platform shell helpers:** `shell_command()` and `shell_command_sync()` in `process_util` abstract `cmd /c` (Windows) vs `sh -c` (Unix) for all runners and hooks
- **Cross-platform process checks:** `is_pid_alive_sync()` and `is_pid_alive_async()` use `kill -0` on Unix (works on both Linux and macOS, unlike `/proc` which is Linux-only) and `tasklist` on Windows
- **Platform-aware install hints:** Dependency detectors now suggest `brew install` on macOS, `apt`/`dnf` on Linux, and `winget install` on Windows instead of hardcoded winget commands
- **Docker runner Unix support:** Raw docker commands and Dockerfile builds now use `sh -c` on Unix instead of `cmd /c`
- **Custom hooks Unix support:** User-defined hooks now execute via `sh -c` on Unix

### Changed
- **Consolidated process utilities:** Duplicated `is_pid_alive` logic in `manager/logs.rs` and `daemon/lifecycle.rs` now delegates to shared `process_util` functions
- **Total tests:** 150 → 158 passing

## [0.20.0] - 2026-03-10

### Added
- **Production-grade Dockerfile generator:** Complete rewrite following Docker official best practices, Astro docs, and Next.js docs
  - **Astro support:** SSG (nginx) and SSR (Node.js runtime) auto-detection via `astro.config` output mode
  - **Next.js standalone mode:** Optimized template using Next.js standalone output for minimal images
  - **Vite/React SPA:** Multi-stage build + nginx for static sites
  - **TypeScript `tsc` bypass:** Auto-detects if `npm run build` invokes `tsc` (e.g., `tsc && vite build`) and calls the bundler directly to avoid strict mode failures in Docker builds. Type-checking belongs in CI, not in container builds
  - **Package manager auto-detection:** pnpm (corepack), yarn, npm — each with correct lockfile and install commands
  - **Non-root users:** All templates use `USER node`, `USER app`, or `USER nonroot` following Docker security best practices
  - **Framework detection by config files:** `astro.config.mjs`, `next.config.js`, `vite.config.ts` take priority over `package.json` deps
  - **`.dockerignore` generation:** Auto-generated per project type when saving a Dockerfile
  - **Node.js version default:** Updated from 20 to 22 LTS
  - 14 unit tests covering all frameworks, tsc bypass, SSR/SSG modes, pnpm, and config file detection
- **Docker URL inference:** Docker services now show clickable `http://localhost:{port}` links on ServiceCards, derived from port mapping config (nginx/static servers don't print URLs to stdout)
- **Dockerfile preview auto-load:** Docker panel auto-generates preview when switching to Dockerfile/Compose tabs
- **Dockerfile regenerate button:** Overwrite existing Dockerfile with a newly generated one based on current project detection

### Fixed
- **Cross-platform compilation:** Added `libc` as Unix-only dependency (`cfg(unix)`) in `void-stack-core` and `void-stack-daemon`. The `libc::kill`/`SIGTERM` calls in the non-Windows `#[cfg]` blocks failed to compile on macOS/Linux because `libc` was missing from `Cargo.toml`
- **Unix unused import warning:** `tracing::warn` conditionally imported only on Windows where `taskkill` error logging uses it
- **Desktop crate marked `publish = false`:** Prevents accidental `cargo install` attempts — desktop requires `cargo tauri build` with frontend assets and icons
- **Docker Build mode CMD override:** Runner no longer appends `service.command` to `docker run` in Build mode — the Dockerfile's own CMD/ENTRYPOINT is used. Previously, commands like `nginx -g daemon off;` were passed as separate shell args, causing `nginx: invalid option: "off;"`
- **Docker image tag sanitization:** Service names with colons (e.g., `docker:void-stack-landing`) are sanitized to valid Docker tag format (colons → dashes)
- **Service removal navigation:** Removing a service no longer triggers `window.location.reload()` — uses custom event to refresh project list while keeping current project selected
- **Docker panel state reset:** Switching projects now properly resets generated Dockerfile/Compose state

### Changed
- **Total Dockerfile tests:** 8 → 14
- **Installation docs:** README now documents per-component `cargo install --git` commands (CLI, TUI, MCP, Daemon) since workspace has multiple binary crates

## [0.19.0] - 2026-03-10

### Added
- **Oxlint integration (Rust-native):** Primary frontend linter for React, Vue, Astro, and Svelte projects. Zero-config, 50-100x faster than ESLint. Auto-detects framework and enables relevant plugins (react, jsx-a11y, vue, import)
- **Vue.js best practices:** ESLint fallback with `eslint-plugin-vue` for deeper Vue-specific rules (`<template>` + `<script>` analysis)
- **Angular best practices:** `ng lint` (angular-eslint) integration with fallback to direct ESLint. Detects `angular.json` and `@angular/core` in package.json
- **Astro best practices:** ESLint fallback with `eslint-plugin-astro` for Astro component linting (frontmatter, HTML template, JSX, directives)
- **Hybrid linting strategy:** Oxlint runs first as fast primary linter, then framework-specific ESLint plugins provide deeper analysis. Both results are merged into unified best practices report
- **15 new unit tests** for Oxlint category mapping, ESLint JSON parsing, Angular category mapping, Astro detection, and Vue detection

### Changed
- **Linter execution order:** Rust-native linters (Oxlint, clippy, ruff, golangci-lint, dart analyze) now run before ESLint-based linters for faster feedback
- **Total tests:** 135 → 150 passing

## [0.18.1] - 2026-03-09

### Added
- **Technology icons on ServiceCards:** Each service card displays an inline SVG icon for its detected technology (Python, Node, Rust, Go, Flutter, Docker, Java, .NET, PHP) with a brand-colored glow effect when the service is running
- **OS-specific target icons:** Target badge now shows Windows logo, Linux/Tux, or Docker whale instead of generic Monitor/Terminal icons
- **Open URL in browser:** Clicking a running service's URL now opens in the system browser via `tauri-plugin-opener` (previously blocked by Tauri's webview security)

### Fixed
- **Card layout overflow:** Long service names, commands, and URLs now truncate with ellipsis instead of breaking the card layout. Target badge no longer gets compressed by long titles
- **Build mode:** Desktop must be built with `cargo tauri build` (not `cargo build --release`) to properly embed the frontend — `cargo build --release` always set Tauri dev mode, causing `ERR_CONNECTION_REFUSED`

## [0.18.0] - 2026-03-09

### Added
- **Docker Runner (Phase 12):** Services with `target = "docker"` now execute inside Docker containers instead of falling back to local execution. Four modes (auto-detected):
  - **Raw command:** `command = "docker compose up postgres"` → runs as-is
  - **Image reference:** `command = "redis:7-alpine"` → auto `docker run --name vs-<service> --rm <image>`
  - **Compose auto-detect:** If project has `docker-compose.yml` and service name matches a compose service → auto `docker compose up <service>` (uses existing compose files or files generated by Docker Intelligence)
  - **Dockerfile build:** Regular command + Dockerfile in working_dir → auto `docker build` + `docker run`
- `DockerConfig` struct on `Service` model for per-service Docker configuration: `ports`, `volumes`, `extra_args`
- Container lifecycle management: auto-cleanup of stopped containers, graceful `docker stop -t 10`, container naming convention `vs-<service-name>`
- Example config in `example-void-stack.toml` with Docker image and Dockerfile build examples
- **Docker Runner across all interfaces:**
  - **CLI:** `--port`, `--volume`, `--docker-arg` flags on `void add-service`; Docker config shown in `void list`
  - **MCP:** `docker_ports`, `docker_volumes`, `docker_extra_args` params on `add_service` tool; Docker info in `list_projects`
  - **Desktop:** "Add Service" form with Windows/WSL/Docker target selector; Docker ports and volumes fields; ServiceCard shows Docker config badges
  - **Desktop — Docker Auto-Import:** "Import from Docker" button auto-detects services from `docker-compose.yml` or `Dockerfile`. Compose imports as a single `docker compose up` service (launches all containers together). Preview shows aggregated ports, volumes, and container list. Services get `docker:` prefix to coexist with local services
  - **TUI:** Already works through ProcessManager + target column display
- **Remove Service:** Delete individual services from a project via desktop UI (trash button on each ServiceCard with two-step confirmation), backed by `remove_service()` in core
- **Process exit watcher:** Background task monitors service PIDs — when a process dies unexpectedly, status updates to FAILED and error appears in logs (no more silent failures)

## [0.17.0] - 2026-03-09

### Fixed
- **Draw.io rendering**: Backend now returns Draw.io XML per section (architecture, API routes, DB models) when format is "drawio" — previously always returned Mermaid text regardless of format
- Individual page generation functions (`generate_architecture`, `generate_api_routes`, `generate_db_models`) in Draw.io module for per-section rendering
- Combined multi-page `.drawio` file still auto-saved for external editors
- **Security audit false positives**: Reduced from 83% (5/6) to 0% false positive rate
  - Skip self-referencing files (audit detection patterns, security regex, docker templates)
  - Filter lines containing regex metacharacters (detection pattern definitions)
  - Filter template/format string lines (placeholders, `format!()`, `push_str()`)
  - Filter Rust raw string literals and `Regex::new` patterns
  - Filter JSX/TSX elements and object literal mappings with code identifiers
  - Risk score dropped from 25/100 to 2/100 on self-analysis
- **Draw.io dark theme readability**: Added `toDarkFill()` / `toDarkStroke()` color mapping for SVG renderer — text now readable on dark backgrounds
- **Console windows flashing on Windows**: Centralized `HideWindow` trait (`process_util.rs`) with `CREATE_NO_WINDOW` flag applied to all 18 `Command::new` call sites across detectors, audit, analysis, hooks, and runner
- **Best practices timeout on large workspaces**: Increased clippy timeout to 180s, removed `--all-targets` flag, fixed stale timeout message (said 300s when actual was different)
- **Git history secrets false positive**: Commits that refactor security/audit detection code (containing "password", "token" as regex patterns) were flagged as leaked credentials. Added `is_false_positive_commit()` filter that skips commits matching 2+ indicators (refactor, audit, vuln_pattern, test, etc.)
- **Draw.io DB models white-on-white**: Added `#ffffff` → `#141820` and `#d6d6d6` → `#3a3a4a` dark theme color mappings for DB model field cells

### Changed
- **Unified diagram scanners** — Draw.io and Mermaid now use the same analysis pipeline. Route scanning (`api_routes::scan_raw`) and DB model scanning (`db_models::scan_raw`) are shared. Draw.io previously had its own limited Python/Node-only scanners (~400 LOC) that missed gRPC, Protobuf, Prisma, Drift, GORM, Swagger. All duplicated code removed.
- **Refactored CLI** (`void-stack-cli`): Extracted God Class `main.rs` (1202 LOC, 25 functions) into 6 focused modules: `commands/project.rs`, `commands/service.rs`, `commands/analysis.rs`, `commands/docker.rs`, `commands/deps.rs`, `commands/daemon.rs`. Main reduced to ~250 LOC.
- **Refactored MCP server** (`void-stack-mcp`): Extracted God Class `server.rs` (1197 LOC, 35 functions) into 10 tool modules: `tools/projects.rs`, `tools/services.rs`, `tools/analysis.rs`, `tools/diagrams.rs`, `tools/docker.rs`, `tools/docs.rs`, `tools/debt.rs`, `tools/space.rs`, `tools/suggest.rs`. Server reduced to ~340 LOC with `#[tool]` stubs delegating to modules.
- **Refactored `analyze_best_practices`** (CC=42→~15): Table-driven linter registry with `LinterDef` struct, individual runner wrappers, and `merge_linter_output` helper. Eliminates duplicated 5-way if-chains.
- **Refactored `cmd_analyze`** (CC=41→~10): Extracted 11 helper functions for printing summaries, snapshot handling, cross-project analysis, and best practices formatting.
- **Refactored `manager.rs`** (30 functions): Split into 4 submodules: `process.rs` (start/stop/is_running), `state.rs` (status tracking), `logs.rs` (log readers), `url.rs` (URL detection). Public API unchanged.
- **Refactored `vuln_patterns.rs`** (789 LOC, 32 functions): Split into 5 category submodules: `injection.rs`, `xss.rs`, `network.rs`, `crypto.rs`, `config.rs`. All 16 tests preserved.
- **Default AI model** changed from `qwen2.5-coder:7b` to `qwen2.5:7b` (more commonly available). Error on missing model now lists available models.

## [0.16.1] - 2026-03-09

### Fixed
- **XSS vulnerability**: Mermaid SVG output now sanitized with DOMPurify before innerHTML injection
- **Mermaid error fallback**: Uses `textContent` instead of `innerHTML` to prevent injection

### Added
- **Draw.io native renderer**: Custom SVG renderer parses mxGraphModel XML and renders cells inline with cyberpunk dark theme, zoom controls, and DOMPurify sanitization
- Draw.io renderer falls back gracefully to formatted XML display if rendering fails

## [0.16.0] - 2026-03-09

### Added
- **AI-Powered Contextual Suggestions (Phase 7g):**
  - New `ai` module in `void-stack-core` with provider abstraction, prompt builder, and response parser
  - Ollama provider: calls local LLM API (`/api/generate`) with configurable model and base URL
  - Prompt builder: converts `AnalysisResult` (anti-patterns, complexity hotspots, circular deps, coverage, architecture) into focused Spanish-language prompts for the LLM
  - Response parser: extracts structured suggestions (category, title, description, affected files, priority) from free-form LLM responses
  - AI config stored in `%LOCALAPPDATA%\void-stack\ai.toml` (provider, model, base_url)
  - CLI: `void suggest <project> [--model <model>] [--service <svc>] [--raw]` — analyzes project then generates AI suggestions via Ollama
  - MCP: `suggest_refactoring` tool — runs analysis + AI suggestions; falls back to raw analysis context if Ollama unavailable
  - Desktop: "AI Suggestions" button in Analysis panel — shows suggestions inline with priority badges and affected files; graceful fallback when Ollama is not running
  - Graceful degradation: if Ollama is not available, returns the structured analysis context so the AI assistant (or user) can reason about it directly
  - 9 new tests for prompt builder, suggestion parser, config serialization, and category detection

## [0.15.0] - 2026-03-09

### Added
- **Docker Intelligence (Phase 11):**
  - Parse existing Dockerfiles: extract stages, base images, exposed ports, CMD/ENTRYPOINT, ENV, WORKDIR
  - Parse docker-compose.yml/yaml: extract services, images, port mappings, volumes, environment, depends_on, healthchecks
  - Auto-classify compose services by kind: database, cache, queue, proxy, worker, app
  - Generate Dockerfiles for Python (FastAPI/Django/Flask), Node (Next/Vite/Express), Rust (cargo-chef), Go (distroless), Flutter (web+nginx)
  - Generate docker-compose.yml from project services with auto-detected infrastructure (PostgreSQL, MySQL, MongoDB, Redis, RabbitMQ) from dependency manifests and .env files
  - CLI: `void docker <project> [--generate-dockerfile] [--generate-compose] [--save]`
  - MCP: `docker_analyze` and `docker_generate` tools for AI assistant integration
  - Desktop: new Docker tab with analysis view, Dockerfile generator, and Compose generator with save-to-disk
  - 17 new tests for Dockerfile parsing, compose parsing, Dockerfile generation, and compose generation
- **Infrastructure Intelligence (Terraform, Kubernetes, Helm):**
  - Terraform HCL parser: detect AWS (RDS, ElastiCache, S3, Lambda, SQS, SNS, ECS), GCP (Cloud SQL, Redis), Azure (PostgreSQL, Redis) resources from `.tf` files
  - Extract resource attributes (engine, version, runtime, instance class) via regex-based HCL parsing
  - Kubernetes manifest parser: detect Deployments, Services, Ingress, StatefulSets, ConfigMaps, Secrets from YAML files in `k8s/`, `kubernetes/`, `manifests/`, `deploy/` directories
  - Extract container images, ports, replicas, and namespaces from K8s resources
  - Helm chart parser: parse `Chart.yaml` for chart name, version, and dependencies (bitnami/postgresql, bitnami/redis, etc.)
  - Architecture diagrams now include Terraform infrastructure subgraph, Kubernetes subgraph, and Helm chart dependencies
  - Mermaid shapes: `[(database)]` for DBs, `{{compute}}` for Lambda/ECS, `[/storage/]` for S3, `[[queue]]` for SQS/SNS
  - CLI, MCP, and Desktop Docker tab automatically display detected infrastructure
  - Desktop DockerPanel shows Terraform, Kubernetes, and Helm sections with resource cards
  - 13 new tests for Terraform, Kubernetes, and Helm parsing

## [0.14.1] - 2026-03-09

### Added
- **Swagger/OpenAPI integration for API route diagrams:**
  - Parses `swagger.json`, `openapi.yaml`, and swagger-jsdoc YAML fragments
  - Enriches detected routes with `summary` and `tag` from API documentation
  - Adds routes found only in Swagger docs but not detected by code scanning
  - Case-insensitive recursive scanning of docs/swagger/openapi directories
- **Internal API route detection:**
  - Routes with `/internal` in their path are automatically classified as internal
  - Diagram separates public and internal routes into distinct subgraphs
  - Visual connection between public and internal API groups
- **HTTP method color coding** in API route diagrams (🟢 GET, 🟡 POST, 🟠 PUT/PATCH, 🔴 DELETE, 🔵 WS, 🟣 RPC)

### Changed
- **External service detection refactored** — no more hardcoded pattern matching:
  - Extracts actual URLs from source code string literals (language-agnostic)
  - Cross-references `localhost:PORT` URLs with project services to identify internal service-to-service calls
  - Parses `.env` file values for localhost URLs and maps ports to known services
  - External domains classified dynamically by domain name instead of filename matching
  - Env var references (`*_URL`, `*_API`, `*_ENDPOINT`) detected across all languages
- **Case-insensitive directory scanning** in API route detection using `find_subdirs_ci()`

- **Expandable debt metrics in Desktop**:
  - God Classes, Complex Functions, Anti-patterns, and Circular Deps rows are now collapsible
  - Clicking a metric with detail expands to show file paths, line numbers, complexity scores, and cycle paths
  - Chevron indicators (▶/▼) show which metrics have expandable detail
  - Severity-colored detail items (red/amber for high/medium severity)
  - Animated expand/collapse transitions

### Improved
- **DB Models Draw.io layout**: BFS ordering groups FK-related models in adjacent positions, dynamic row heights, curved edge routing
- **Desktop UI visual hierarchy redesign**:
  - Section titles: 18px bold with cyan glow text-shadow
  - Architecture pattern: hero gradient card (36px cyan→purple)
  - KPI metrics: 32px bold numbers for LOC, modules, anti-patterns
  - Service cards: green glow box-shadow on running services
  - Security risk score: enlarged circle (100px) with glow
  - Anti-patterns: severity-specific left borders with gradient backgrounds (red/amber/cyan)
  - Mini-log preview: greener text for better visibility

### Fixed
- API route diagram compilation errors (Route struct fields, missing `route_color()` function)
- Localhost URLs incorrectly ignored in architecture diagrams — now properly detected as internal service calls
- DB Models Draw.io diagram: models scattered without relationship proximity — now grouped by FK adjacency

## [0.14.0] - 2026-03-09

### Added
- **WSL project support (UNC paths):**
  - All tools (diagrams, analysis, audit, check) now work with WSL projects
  - Projects stored as UNC paths (`\\wsl.localhost\<distro>\...`) accessible by Windows `std::fs`
  - Runner auto-converts UNC → Linux path with correct distro (`wsl -d Ubuntu -e bash -c`)
  - `resolve_wsl_path()` handles Git Bash path mangling, UNC, and pure Linux paths
  - CLI: `--distro` flag for `void add` and `void scan` commands
  - MCP: `wsl` and `distro` parameters on `add_project` tool
  - Desktop: WSL browser passes UNC path directly, auto-detected as WSL target
- **Draw.io diagram format in desktop app:**
  - Format selector (Draw.io / Mermaid) with Draw.io as default
  - "Save" button writes `.drawio` file to project directory
  - XML code view with copy button
  - Info hint for opening with diagrams.net / VS Code Draw.io extension
- **Flutter/Dart diagram support:**
  - Drift table scanning (`extends Table` with `IntColumn`, `TextColumn`, etc.)
  - Protobuf message parsing for DB model diagrams
  - gRPC service/rpc method detection for API route diagrams
  - Flutter/Dart service detection in architecture diagrams (`flutter run`, `pubspec.yaml`)
- **Custom WSL file browser** in desktop app:
  - Lists WSL distros via `wsl --list --quiet` (UTF-16LE parsing)
  - In-app folder browser using `std::fs::read_dir` on `\\wsl.localhost\` UNC paths
  - Directory tree navigation with breadcrumb, back button, and folder selection
  - Bypasses Windows native dialog limitation that doesn't support WSL UNC paths
- **Copy buttons** on results (diagrams, security audit, technical debt)
- **Educational tooltips** (InfoTip component) on security categories and debt metrics
- **Diagram zoom controls** — +/- buttons with percentage display on Mermaid renders
- **Re-analyze button** in Technical Debt panel
- **i18n**: all new strings in English and Spanish

### Changed
- **Rebrand**: devlaunch → void-stack (crate names, binary names, config paths, proto package)
- **UI readability overhaul** following Material Design 3 typography guidelines:
  - Base body font: 14px with 1.5 line-height
  - Text contrast increased: primary #cdd4e0, secondary #8a97ab (WCAG AA compliant on dark BG)
  - All font sizes bumped +2px, buttons padding increased
  - Tab bar: compact sizing to fit all 9 tabs
- **Diagram rendering fix**: subgraph IDs prefixed with `proj_` to avoid collision with node IDs
- **Diagram labels**: `\n` replaced with `<br/>` for proper line breaks in Mermaid
- **erDiagram fix**: FK/M2M types output as mermaid key annotations (`string field FK`) instead of invalid types
- **Recursive scanner limits**: Drift/Proto scanners now skip heavy dirs (node_modules, .git, build, target) and limit depth to 3-4 levels

### Fixed
- WSL projects failing all analysis/diagram/audit tools (stored Linux path instead of UNC)
- Mermaid erDiagram not rendering when models contain FK-typed fields
- Diagram generation hanging on WSL projects due to unbounded directory recursion
- Mermaid diagrams showing raw code instead of rendered SVG when project/service names collide
- Tooltips invisible due to panel `overflow-y: auto` clipping absolute-positioned elements
- Tabs cut off on smaller/standard resolutions

## [0.13.0] - 2026-03-09

### Added
- **Best Practices Analyzer** — delegates to native ecosystem linters for unified reporting:
  - `react-doctor` for React/TS/Next.js (60+ rules, score 0-100)
  - `ruff` for Python (500+ rules, JSON output, S-prefix filtered to avoid audit overlap)
  - `cargo clippy` for Rust (pedantic + perf + complexity lints)
  - `golangci-lint` for Go (errcheck, govet, staticcheck, gosimple, etc., gosec filtered)
  - `dart analyze` / `flutter analyze` for Dart/Flutter (--machine format)
- `--best-practices` flag on `void analyze` command
- `--bp-only` flag to skip architecture analysis and only run linters
- `best_practices` parameter on MCP `analyze_project` tool
- Best Practices collapsible section in desktop Análisis tab with score circle, tool chips, filter buttons, finding cards
- 5 new dependency detectors: Ruff, Clippy, golangci-lint, Flutter Analyze, react-doctor
- Score formula: 100 - (Important×5 + Warning×2 + Suggestion×0.5), per-tool sub-scores
- 10 new unit tests for tool output parsing

### Fixed
- **False positive reduction in security audit:**
  - Rust `Command::new()` with safe argument arrays no longer flagged as command injection
  - Go `exec.Command()` with safe arguments no longer flagged
  - `innerHTML` in mermaid/chart.js rendering contexts reduced to Low severity
  - Files with `#[cfg(test)]` or `#[test]` treated as test files for secret detection
  - Files in `audit/` directory treated as test context for secret detection

## [0.12.0] - 2026-03-09

### Added

#### Extended Security Patterns (Phase 9)
- **SQL Injection detection**: Python f-string/format/execute concat, JS template literals in SQL queries
- **Command Injection detection**: Python subprocess shell=True, os.system, eval/exec; JS child_process exec/spawn, eval; Go exec.Command; Rust Command::new with variables
- **Path Traversal detection**: Python open/send_file/FileResponse with unvalidated input; JS fs.readFile/res.sendFile with req params
- **Insecure Deserialization**: Python pickle.loads, yaml.load without SafeLoader, marshal, jsonpickle; JS unserialize
- **Weak Cryptography**: md5/sha1 in security contexts, Math.random/random module for tokens, weak ciphers (DES/RC4), hardcoded IVs
- **XSS detection**: innerHTML/outerHTML assignment, document.write, insertAdjacentHTML, eval, new Function, dangerouslySetInnerHTML (Low severity)
- **SSRF detection**: HTTP requests with variable URLs inside route handlers (Python requests/httpx, JS fetch/axios, Go http.Get)
- **Exposed Debug Endpoints**: routes matching /debug, /actuator, /phpinfo, /.env, /metrics, /heapdump
- **Secrets in Git History**: git log search for deleted commits containing password, secret, AKIA, api_key, token
- **9 new FindingCategory variants**: SqlInjection, CommandInjection, InsecureDeserialization, WeakCryptography, XssVulnerability, Ssrf, ExposedDebugEndpoint, SecretInGitHistory (PathTraversal already existed)
- **Markdown report sections**: separated into "Secrets, Configs y Dependencias" and "Code Vulnerability Patterns"
- **False positive reduction**: skip .min.js, reduce severity in test/spec/mock files, context-aware crypto flagging
- **16 unit tests** covering all new categories plus severity reduction and minified file skipping

## [0.11.0] - 2026-03-09

### Added

#### Security Audit (Phase 8)
- **`audit` module** in void-stack-core: full security scanning engine
- **Dependency vulnerability scanning**: `npm audit`, `pip-audit`, `cargo audit`, `govulncheck` — parses JSON output, maps to findings with severity
- **Hardcoded secrets detection**: 12 patterns (AWS keys, GitHub tokens, Stripe keys, JWT secrets, DB URLs, Google API keys, Slack tokens, SendGrid, private keys, generic API keys/passwords)
- **Insecure config detection**: Django DEBUG=True, Flask debug, CORS wildcard, 0.0.0.0 binding, missing .env.example, .env not in .gitignore, Dockerfile issues (root user, :latest tag, COPY without .dockerignore), suspicious npm install scripts
- **Risk score**: weighted formula (critical=40, high=20, medium=5, low=1), capped at 100
- **Markdown report generation**: `void-stack-audit.md` with severity icons, categories, file locations, remediation steps
- **CLI command**: `void audit <project> [-o output.md]`
- **MCP tool**: `audit_project` for Claude Desktop/Code integration
- **Desktop tab**: "Seguridad" with risk score circle, severity count badges, finding cards with category/severity/file/remediation
- **Monorepo support**: scans subdirectories for package.json, requirements.txt, Cargo.lock, go.sum
- 4 unit tests: empty project, hardcoded API key detection, debug mode detection, risk score calculation

## [0.10.0] - 2026-03-09

### Added

#### Disk Space Scanner
- **`space` module** in void-stack-core: scan project dirs for heavy folders (node_modules, venv, target, build, dist, .dart_tool, __pycache__, .next, .nuxt)
- **Global cache scanning**: npm, pip, Go modules, Cargo registry, Dart pub, Gradle, Ollama, HuggingFace, LM Studio, PyTorch hub
- **Safe deletion** with allow-list validation and human-readable size formatting
- **Espacio tab** in desktop UI: scan + delete project and global space, grouped sections, category badges, total recoverable summary

#### ORM Diagram Support
- **Sequelize scanner**: detects `sequelize.define()`, `Model.init()`, `class extends Model` patterns with DataTypes mapping
- **GORM scanner**: detects Go structs with `gorm.Model` embed or `gorm:"..."` tags, maps Go types

#### New Detectors
- **Go detector** (`golang.rs`): checks `go version`, `go.mod` presence
- **Flutter detector** (`flutter.rs`): checks `flutter --version`, `dart --version`, `pubspec.yaml` presence
- **Flutter ProjectType**: detection via `pubspec.yaml`, default command `flutter run`

## [0.9.0] - 2026-03-09

### Added

#### Desktop UI (`void-stack-desktop`) — Phase 6
- **Tauri v2 desktop application** with React + TypeScript frontend
- **Mission Control dark theme**: JetBrains Mono typography, electric cyan accents, scan-line texture, animated system pulse
- **6 tabs**: Servicios, Registros, Dependencias, Diagramas, Análisis, Docs
- **Servicios tab**: service cards with status (running/stopped/failed), PID, uptime, URL, start/stop controls
- **Registros tab**: live log viewer with service selector, auto-scroll toggle, monospace output
- **Dependencias tab**: dependency check results table with status, version, fix hints
- **Diagramas tab**: Mermaid diagram rendering with render/code toggle for architecture, API routes, and DB models
- **Análisis tab**: architecture pattern with confidence bar, layer distribution chart, anti-pattern cards with severity badges, cyclomatic complexity table, coverage bar
- **Docs tab**: renders project README.md with full markdown styling, dropdown for other doc files (CHANGELOG, etc.)
- **Project management**: add/remove projects from the sidebar, delete button per project
- **Project switch reset**: switching projects clears all tab data (deps, diagrams, analysis, logs, docs)
- **Installers**: MSI and NSIS setup executables generated automatically
- 14 Tauri commands wrapping void-stack-core: list_projects, add_project, remove_project_cmd, get_project_status, start_all, stop_all, start_service, stop_service, get_logs, check_dependencies, generate_diagram, analyze_project_cmd, read_project_readme, list_project_docs, read_project_doc

### Fixed
- **Process stop verification**: `stop_one()`/`stop_all()` now verify process death with `is_running()` retry and update state immediately
- **Console window flashing**: `CREATE_NO_WINDOW` flag applied to all Windows Command spawns (service start, taskkill, tasklist)
- **Stale tab data on project switch**: all cached tab data resets when selecting a different project

### Security
- **Centralized security module** (`security.rs`): sensitive file deny-list with `is_sensitive_file()`, `read_env_keys()`, `env_keys_contain()`
- Applied to: ollama detector, architecture/drawio diagrams, analyzer imports, MCP tools, desktop docs viewer
- **Crate relationship detection**: architecture diagrams detect Cargo.toml workspace members and internal dependencies

## [0.7.1] - 2026-03-08

### Security
- **Sensitive file protection**: centralized `security` module with deny-list of sensitive files
  - `.env`, credentials, private keys, secrets files are never read in full
  - `.env` files scanned by key names only (values never exposed)
  - Applied to: detector/ollama, diagram/architecture, diagram/drawio, analyzer/imports, MCP read_project_docs
- **MCP tool hardening**: `read_project_docs` now blocks access to sensitive files (secrets.json, .env, etc.)

### Added
- **Crate relationship detection**: architecture diagrams now detect Cargo.toml workspace members and their internal dependencies, rendering them as a "Rust Crates" subgraph with dependency arrows

## [0.7.0] - 2026-03-08

### Added

#### Code Analysis (`void-stack-core/analyzer`) — Phase 7b
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

#### Test Coverage Visualization — Phase 7c
- **4 coverage format parsers** with auto-detection:
  - **LCOV** (Flutter `flutter test --coverage`, genhtml)
  - **Cobertura XML** (pytest-cov, cargo-tarpaulin, generic)
  - **Istanbul JSON** (c8, nyc — `coverage-summary.json` and `coverage-final.json`)
  - **Go cover profiles** (`go test -coverprofile`)
- **Visual coverage in docs**: overall bar, per-file table with color indicators
- **CLI output**: coverage percent, lines covered, tool name

#### Cyclomatic Complexity — Phase 7d
- **Per-function complexity analysis**: counts branching points (if, for, while, match, try/except, elif, &&, ||, ternary)
- Supported languages: Python, JavaScript/TypeScript
- Complexity section in markdown docs: top complex functions table with file, line, score
- CLI output: max complexity function, count of complex functions (>=10)

#### Technical Debt Tracking — Phase 7e
- **Analysis snapshots**: saved to `.void-stack/history/` as timestamped JSON files
- **`--compare` flag**: shows trend vs previous snapshot (improving/stable/degrading)
- **`--label` flag**: tag snapshots with version, git tag, etc.
- Comparison report in markdown: LOC delta, anti-pattern delta, complexity delta, coverage delta

#### Cross-Project Coupling — Phase 7f
- **`--cross-project` flag**: detects dependencies between registered Void Stack projects
- Matches external deps against project identifiers (name, dir, package.json name, pyproject.toml name)
- Mermaid diagram of inter-project relationships in markdown output

#### CLI
- **`void analyze <project> [-o file] [-s service] [--compare] [--cross-project] [--label v1.0]`**
- **`void diagram <project> [-f drawio|mermaid] [-o file]`**: format selection flag

#### MCP Server
- **`analyze_project`** tool: returns markdown analysis with architecture patterns, anti-patterns, and cyclomatic complexity
- **`generate_diagram`** tool: supports `format` parameter ("mermaid" or "drawio")

## [0.6.0] - 2026-03-08

### Added

#### Mermaid Diagram Generation (`void-stack-core`) — Phase 7
- **Architecture diagrams** (`graph TB`): auto-detects service types (Frontend/Backend/Worker), ports, connections (frontend→backend), and external services (PostgreSQL, Redis, Ollama, AI APIs, AWS S3)
- **API route diagrams** (`graph LR`): scans FastAPI/Flask decorators (`@app.get`, `@router.post`) and Express routes (`app.get`, `router.post`) with method-colored badges
- **DB model diagrams** (`erDiagram`): detects SQLAlchemy (Column + Mapped), Django (models.Model), and Prisma schema models with field types

#### CLI
- **`void diagram <project> [-o file]`**: generate all diagrams to stdout or file

#### MCP Server
- **`generate_diagram`** tool: returns Mermaid markdown for architecture, API routes, and DB models

## [0.5.0] - 2026-03-08

### Added

#### Dependency Detection (`void-stack-core`) — Phase 4
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
- **`void check <project>`**: verify all dependencies before starting services
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

#### MCP Server (`void-stack-mcp`) — Phase 3
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

#### Smart Command Detection (`void-stack-core`)
- **Python framework auto-detection**: Analyzes source files to determine the correct start command
  - **FastAPI/Starlette**: `uvicorn module:app --host 0.0.0.0 --port 8000`
  - **Flask**: `flask --app varname run --port 5000`
  - **Django**: `python manage.py runserver`
  - **Self-starting** (has `uvicorn.run()` in `__main__`): `python filename.py`
  - **Generic `__main__`**: `python filename.py`
- **App variable detection**: Finds the ASGI/WSGI variable name (e.g., `server = FastAPI()` → `server`)
- **Candidate file scanning**: Checks `main.py`, `app.py`, `server.py`, `run.py`, `manage.py` in order
- **Per-project-type defaults**: Node → `npm run dev`, Rust → `cargo run`, Go → `go run .`, Docker → `docker compose up`

#### TUI (`void-stack-tui`)
- **Multi-project dashboard**: Shows all registered projects from global config
- **Three-panel layout**: Projects list (left), Services table (right), Logs (bottom)
- **Tab/Shift+Tab** to cycle between panels, arrow keys to navigate
- **Project indicator**: ● green (has running services) / ○ gray (all stopped)
- **Optional filter**: `void-stack-tui [project_name]` for single-project view
- Stops all services across ALL projects on quit

---

## [0.3.0] - 2026-03-08

### Added

#### Core (`void-stack-core`)
- **Log capture**: Piped stdout/stderr from child processes via `BufReader::lines()` with background tokio tasks
- **URL auto-detection**: Regex-based detection of `http://localhost:PORT` URLs from service output, stored in `ServiceState.url`
- **ANSI stripping**: Strip terminal escape codes before URL matching (Vite, Next.js colorize URLs)
- **Python virtualenv auto-detection**: Automatically resolves `python` commands to `venv/Scripts/python.exe` (searches working_dir and parent for monorepos)
- **`get_logs()` method**: Retrieve captured log lines per service (up to 5000 lines buffered)

#### CLI (`void-stack-cli`)
- **URL display**: Detected service URLs shown in real-time as services start
- **Continuous polling**: Uses `tokio::select!` to poll for URLs indefinitely while waiting for Ctrl+C (no timeout)

#### TUI (`void-stack-tui`)
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

#### Proto (`void-stack-proto`)
- **Protobuf service definition**: `void_stack.proto` with 10 RPCs (StartAll, StartOne, StopAll, StopOne, GetStates, GetState, RefreshStatus, StreamLogs, Shutdown, Ping)
- **Type conversions**: Bidirectional `From` impls between core types (`ServiceState`, `ServiceStatus`) and protobuf types
- **DaemonClient**: gRPC client implementing `ServiceBackend` trait for transparent daemon mode

#### Daemon (`void-stack-daemon`)
- **gRPC server**: Tonic-based server exposing all ProcessManager operations
- **PID file management**: Write/read/cleanup daemon info in `%LOCALAPPDATA%\void-stack\`
- **Graceful shutdown**: Ctrl+C handler stops all services and removes PID file
- **Daemon subcommands**: `start` (with `--port`), `stop` (graceful via gRPC or fallback kill), `status` (live ping)

#### Core (`void-stack-core`)
- **`ServiceBackend` trait**: Async abstraction over service management, implemented by both `ProcessManager` (direct) and `DaemonClient` (gRPC)

#### CLI (`void-stack-cli`)
- **Dual mode**: `--daemon` flag to connect via gRPC instead of managing processes directly
- **`daemon start|stop|status`** subcommands for daemon lifecycle management
- **`--port`** flag for custom daemon port

#### TUI (`void-stack-tui`)
- **Dual mode**: `--daemon` flag to connect via gRPC to a running daemon
- Refactored to use `ServiceBackend` trait instead of direct `ProcessManager`
- In daemon mode, does not stop services on quit (daemon manages lifecycle)

#### Global Config & Project Management
- **Centralized config** in `%LOCALAPPDATA%\void-stack\config.toml` — manage all projects from one place
- **`void add <name> <path>`** — Register a project with auto-detected services (monorepo aware)
- **`void add-service`** — Add individual services with custom paths to any project
- **`void remove`** — Unregister a project
- **`void list`** — Show all registered projects and their services
- **`void scan`** — Preview what void detects in a directory
- **`--wsl` flag** — Scan and add projects inside WSL with Linux paths
- **Monorepo support** — Scans subdirectories (2 levels deep) for project markers
- **Distributed projects** — Each service has its own absolute `working_dir`, enabling cross-folder grouping
- **WSL scanning** — Single optimized `find` command via WSL for fast detection

### Changed
- CLI and TUI `start`/`stop` commands now use `ServiceBackend` trait for pluggable backends
- CLI redesigned around project-centric workflow (start/stop take project name, not path)

## [0.1.0] - 2026-03-08

### Added

#### Core (`void-stack-core`)
- **Project model**: `Project`, `Service`, `Target` (Windows/WSL/Docker/SSH), `ServiceState`, `ServiceStatus`
- **TOML config**: Load/save `void-stack.toml` project files
- **Project detection**: Auto-detect Python, Node, Rust, Go, Docker projects by file markers
- **Local runner**: Execute processes on Windows (`cmd /c`) and WSL (`wsl -e bash -c`)
- **Process manager**: Start/stop all or individual services, track PIDs, refresh status
- **Pre-launch hooks**: Auto-create Python venv, install dependencies (pip/npm/cargo/go), run builds, custom commands
- **Error handling**: Typed errors with `thiserror`

#### CLI (`void-stack-cli`)
- `void start [service]` — Start all or a specific service
- `void stop [service]` — Stop all or a specific service
- `void status` — Show project info and service list
- `void init` — Generate `void-stack.toml` with auto-detected project type
- `void detect` — Show detected project type

#### TUI (`void-stack-tui`)
- Real-time service dashboard with Ratatui
- Service table: name, target, status (color-coded), PID, uptime
- Log panel: per-service stdout/stderr output
- Keyboard navigation: start/stop services, view logs, refresh, help overlay
- Auto-refresh status every 1 second
- Graceful shutdown: stops all services on quit

#### Testing
- 7 unit tests: config loading, project type detection, runner start/is_running
