# Void Board — void-stack

<!-- void-stack board v1 — one "- **VB-n**" line per task; "- link:" sub-bullets attach files/symbols -->

## Backlog

- **VB-30** Campo origin/tags en registro de proyectos (owned vs third-party/reference) — excluir third-party de board y daily_briefing `prio:medium` `#claude` `#registry` `#dx` `2026-08-10`
- **VB-31** CRUD de planes (.plans/*.md) via MCP y CLI — plan_list / plan_read / plan_write / plan_delete, enlazables a tareas del board `prio:medium` `#mcp` `#board` `#dx` `2026-08-10`
  - link: crates/void-stack-core/src/board.rs
  - link: .plans/VB-29.md

## Doing

## Review

- **VB-29** Transporte HTTP streamable en void-stack-mcp (flag --http) — desbloquea WSL, multi-PC via Tailscale y dashboard `prio:high` `#claude` `#mcp` `#infra` `2026-08-10`
  - link: .plans/VB-29.md
  - link: crates/void-stack-mcp/src/main.rs
  - link: crates/void-stack-mcp/src/cli.rs
  - link: crates/void-stack-mcp/src/http.rs

## Done

- **VB-1** Probar el flujo completo del board desde el MCP `prio:medium` `#test` `#mcp` `2026-07-09`
  - link: docs/superpowers/plans/2026-07-09-board-context-doctor-briefing.md
  - link: crates/void-stack-core/src/board.rs
  - link: CHANGELOG.md
- **VB-28** Filtrar test fixtures y string literals en sync_todos `prio:high` `#bug` `#board` `2026-07-09`
  - link: crates/void-stack-core/src/todosync.rs
  - link: crates/void-stack-core/src/board.rs
