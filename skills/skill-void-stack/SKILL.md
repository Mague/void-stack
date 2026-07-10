---
name: skill-void-stack
description: Working rules for coding sessions on projects managed by void-stack (MCP). Use whenever implementing, testing, or committing in a registered project — covers session bootstrap (session_context), the short test loop (suggest_tests_for_diff), the pre-commit review loop (review_diff), the git-versioned board (board_*), todo-sync, handoff and commit generation.
---

# void-stack session workflow

Standing rules for any coding session on a void-stack-registered project.

## The session loop

| Moment | Tool |
|---|---|
| **Open** | `session_context(project)` — one call replaces the old bootstrap dance |
| **During** | `suggest_tests_for_diff` after each batch · `sync_todos` when you leave TODO/FIXME/HACK markers |
| **Before each commit** | `review_diff` → fix Critical/High → `suggest_commit_message` / `void commit` |
| **Close** | `session_handoff(project, note)` — journal what's half-done |
| **Every morning** | `daily_briefing` — state of all active projects |

## Rule 0 — Session bootstrap (once, at the start)

Call `session_context(project)` (or `void context <project>`) FIRST. It
returns index stats + structural-graph freshness, a README/CLAUDE.md
digest, the current diff with affected symbols, the impact radius of the
changed files, the Doing tasks from BOARD.md **and the last handoff**
(`.void/journal/LATEST.md`) — compact markdown under ~2k tokens. Sections
say "n/a" with the fix (e.g. `index_project_codebase`) when something is
missing.

## Rule 1 — Short test loop (after each batch of edits)

After completing a logical batch of edits, do NOT run the full test suite.
Instead:

1. Call `suggest_tests_for_diff(project)` (or `void suggest-tests <project>`).
2. Run the suggested commands it returns (they are ready to paste:
   `cargo test -p <crate> <name>`, `go test ./pkg -run '^TestX$'`,
   `flutter test <file>`, …).
3. Treat the **Uncovered** section as a TODO: changed symbols with zero
   covering tests need a new test or an explicit reason why not.
4. Run the FULL suite only once, before the final commit of the session.

## Rule 2 — Pre-commit review (before every commit)

1. Call `review_diff(project)` (or `void review <project>`).
2. Address every **Critical/High** finding on changed lines before
   committing — they are scoped to the diff, so they are yours.
3. Include the **Uncovered** list in the commit decision: add tests, or
   state in the commit message why they are acceptable without.
4. Check the **Board** section: when the diff touches files linked to an
   open task, mention the id (VB-n) — or just use `void commit`, which
   does it for you.
5. The payload is compact (≤4k tokens) by design — paste it into the
   reviewer context instead of raw diffs when asking for an LLM review.

## Rule 3 — Close the session

Call `session_handoff {project, note?}` (or `void handoff <p> --note "..."`).
It journals today's commits, the uncommitted diff + touched symbols, the
uncovered list and the in-flight board tasks to
`.void/journal/YYYY-MM-DD-HHmm.md` (+ `LATEST.md`). Commit `.void/journal/`
and the next session — on any machine — starts where this one stopped.

## Board (git-versioned kanban)

`BOARD.md` at the repo root (Backlog / Doing / Review / Done) — plain
markdown, mergeable, GitHub-renderable, synced via git.

- `board_list {project}` — full board as markdown.
- `board_add_task {project, title, priority?, tags?}` — into Backlog as VB-n.
- `board_move_task {project, id, column}`.
- `board_link_task {project, id, query}` — paths/symbols link verbatim,
  natural-language queries resolve through the semantic index. Linked
  tasks surface in `review_diff` and title `void commit` messages.
- `board_archive_done {project, days?}` — old Done → BOARD_ARCHIVE.md.
- `board_history {project, id?}` — every task that EVER existed, from the
  git log of BOARD.md: column transitions per commit (hash + date),
  archived/removed flags. With `id`: one task's full detail + timeline.
- `board_timeline {project, by?, since?}` — ALL work ever done: every
  commit (conventional type/scope/Resolves parsed) plus every board task,
  bucketed by day / week (≈ sprint) / month / year / type / scope.
- `commit_detail {project, hash}` — one commit in full: header, body,
  resolved task ids and per-file additions/deletions.
- `sync_todos {project, clean?}` — mirror `TODO(name)`/`FIXME`/`HACK`
  markers into the Backlog (comment nodes only — never string literals or
  test files; idempotent by content hash; gone markers auto-resolve to
  Done, never silently deleted; `clean` purges tasks from older scans).
  Auto on watch with `[board] todo_sync_on_watch = true` in `.void-config`.
- CLI: `void board <p>` / `add|move|done|link|archive|history|show|timeline`,
  `void todo-sync <p> [--clean]`.
- Desktop: Board panel (Project zone) with drag & drop; click a card for
  its detail modal (metadata, links, git timeline); History toggle lists
  current + archived + removed tasks, with a Group-by selector (day /
  week-sprint / month / year / type / feature area) rendering the full
  work timeline; a toolbar search box live-filters cards, history and
  timeline; ⌘K → "Open the board".

## Commits

`suggest_commit_message {project}` builds a conventional message from the
diff: type from diff shape (docs/test/chore/feat/refactor/fix), scope from
the dominant area weighted by touched symbols, body referencing the board
tasks the diff resolves. The MCP tool NEVER commits; `void commit <p>`
does (moving resolved tasks to Done, BOARD.md in the same commit) and
`--dry-run` previews.

## Verification gates

- `check_contracts {project}` / `void contracts check <p>` — fails
  (exit ≠ 0) when the project consumes a gRPC RPC or REST route its
  producer no longer exposes or whose signature changed; external APIs
  never fail it. Works as a pre-commit/CI gate.
- `check_env {project}` / `void env check <p> [--write]` — env vars the
  code reads vs `.env.example`: used-but-undocumented and
  documented-but-dead. `--write` updates the example preserving comments,
  never copying real values.

## Registry health & daily briefing

- `doctor` (read-only) / `void doctor [--fix] [--json]` — duplicates,
  nested projects, dead paths, broken working_dirs, orphan indexes,
  stale indexes/graphs (>7 days). Fixes apply interactively in the CLI.
- `daily_briefing {projects?, save?}` / `void briefing` — per active
  project: services, debt trend, NEW audit findings only, dependency
  CVEs, contract drift, dead-code count, Doing/Review tasks. Manage with
  `void briefing active <p> on|off`; daemon schedule via
  `void briefing schedule HH:MM` (saved to `briefings/YYYY-MM-DD.md`).
  Desktop: Briefing panel in the Run zone (all active projects, or just
  the selected one).
- `void bootstrap export|import` — portable registry (relative paths, no
  secrets) to provision a new machine; import validates paths and reports
  what's missing.

## Supporting tools

- `build_structural_graph` first if any tool reports a missing graph.
- `get_impact_radius` for a wider blast-radius view than review_diff's
  depth-2 summary.
- `audit_project` for the full project audit (review_diff only scans the
  changed lines).

## Registered projects (snapshot 2026-07-09)

Regenerate with `void list`; prune dead entries with `void doctor --fix`.

- **void-stack** — `/Users/maguedev/workspace/2026/void-stack`
- **void-stack-landing** — `/Users/maguedev/workspace/2026/void-stack-landing`
- **enmanuel.dev** — `/Users/maguedev/workspace/2026/enmanuel.dev`
- **iunci-flutter** — `/Volumes/SSD-EXTERNO/workspace/Flutter/iunci`
- **iunci.app** — `/Users/maguedev/workspace/webs/iunci.app`
- **iunci.store** — `/Users/maguedev/workspace/webs/iunci.store`
- **iunci-demo** — `/Users/maguedev/workspace/videos/iunci-demo`
- **seosnap** — `/Volumes/SSD-EXTERNO/workspace/Flutter/seosnap`
- **uap_rag** — `/Volumes/SSD-EXTERNO/workspace/uap_rag`
- **glowing-robot** — `/Users/maguedev/Documents/workspace/2025/glowing-robot`
- **siarus-frontend** — `/Users/maguedev/Documents/workspace/siarus/siarus-frontend`
- **iws-core-api** — `/Users/maguedev/workspace/work-2025/forks/iws-core-api`
- **core-api** — `/Users/maguedev/workspace/work-2025/forks/core-api`
- **core** — `/Users/maguedev/workspace/work-2025/forks/core`
- **iph-front-core-unified** — `/Users/maguedev/workspace/work-2025/forks/iph-front-core-unified`
- **iph-front-core-unified-fork** — `/Users/maguedev/Documents/workspace/work-2025/forks/front/iph-front-core-unified`

21 additional entries point at paths that no longer exist (old
`~/Documents/workspace` tree) — run `void doctor --fix` to clean them up.
