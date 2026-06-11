---
title: "Docs: complete .void-config reference"
labels: ["good first issue"]
---

**Context**: `.void-config` (TOML) supports `[index]` (ignore, ef_search), `[audit]` (suppress), `[analysis]` (cc_threshold, fat_controller_loc), `[diagram]`, `[ai]` — only fragments are documented.

**Files to touch**: `docs/config.md` (exists, incomplete) — document every section of `crates/void-stack-core/src/project_config.rs` with defaults and one example each.

**Acceptance criteria**: every pub field of `ProjectConfig` appears in docs/config.md with its default; README links to it.
