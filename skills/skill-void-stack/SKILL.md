---
name: skill-void-stack
description: Working rules for coding sessions on projects managed by void-stack (MCP). Use whenever implementing, testing, or committing in a registered project — covers session bootstrap (session_context), the short test loop (suggest_tests_for_diff), the pre-commit review loop (review_diff), and the git-versioned board (board_*).
---

# void-stack session workflow

Standing rules for any coding session on a void-stack-registered project.

## Rule 0 — Session bootstrap (once, at the start)

Call `session_context(project)` (or `void context <project>`) FIRST. One
call replaces the old get_index_stats → read_all_docs → git diff →
get_impact_radius dance: it returns index stats + structural-graph
freshness, a README/CLAUDE.md digest, the current diff with affected
symbols, the impact radius of the changed files, and the Doing tasks
from BOARD.md — compact markdown under ~2k tokens. Sections say "n/a"
with the fix (e.g. `index_project_codebase`) when something is missing.

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

Example:
```
suggest_tests_for_diff {"project": "void-stack"}
→ Suggested tests (3 for 7 changed symbols)
  - test_stop_one_waits_for_child_exit — manager/process.rs:441 (hop 1)
  ...
→ Run:  cargo test -p void-stack-core test_stop_one_waits_for_child_exit
```

## Rule 2 — Pre-commit review (before every commit)

Before each commit:

1. Call `review_diff(project)` (or `void review <project>`).
2. Address every **Critical/High** finding on changed lines before
   committing — they are scoped to the diff, so they are yours.
3. Include the **Uncovered** list in the commit decision: add tests, or
   state in the commit message why they are acceptable without.
4. Check the **Board** section: when the diff touches files linked to an
   open task, mention the id (VB-n) in the commit message and move the
   task (`board_move_task`) if the diff completes it.
5. The payload is compact (≤4k tokens) by design — paste it into the
   reviewer context instead of raw diffs when asking for an LLM review.

## Board (git-versioned kanban)

Each project can carry a `BOARD.md` at its root (Backlog / Doing /
Review / Done). It's plain markdown — mergeable, GitHub-renderable, and
synced across machines via git.

- `board_list {project}` — full board as markdown.
- `board_add_task {project, title, priority?, tags?}` — into Backlog as VB-n.
- `board_move_task {project, id, column}` — also how tasks reach Done.
- `board_link_task {project, id, query}` — attach files/symbols; paths and
  symbols link verbatim, natural-language queries resolve through the
  semantic index. Linked tasks then surface in `review_diff`.
- `board_archive_done {project, days?}` — old Done → BOARD_ARCHIVE.md.
- CLI: `void board <project>` and `void board add|move|done|link|archive`.
- Desktop: Board panel (Project zone) with drag & drop; ⌘K → "Open the board".

## Registry health & daily briefing

- `doctor` (MCP, read-only) / `void doctor [--fix] [--json]` — detects
  duplicate registrations, projects nested inside other projects, dead
  paths, broken service working_dirs, orphan semantic indexes, and
  indexes/graphs staler than 7 days. Apply fixes interactively from the
  CLI only.
- `daily_briefing {projects?, save?}` / `void briefing` — consolidated
  report for the active projects (services, debt trend, NEW audit
  findings since the last run, dead-code count, Doing/Review tasks).
  Manage the list with `void briefing active <project> on|off`; schedule
  daily runs inside the daemon with `void briefing schedule HH:MM`.

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
