---
title: "CLI polish: global --no-color flag"
labels: ["good first issue"]
---

**Context**: `void stats` and `void audit` print ANSI colors unconditionally; piping to files or CI logs gets escape garbage.

**Files to touch**
- `crates/void-stack-cli/src/main.rs`: global `--no-color` flag (and respect `NO_COLOR` env var).
- `crates/void-stack-cli/src/commands/stats.rs` + `analysis/audit.rs`: thread the flag (or a small `colors_enabled()` helper).

**Acceptance criteria**: `NO_COLOR=1 void stats` and `void stats --no-color` emit no `\x1b[` sequences (assert in a test on the formatting helper).
