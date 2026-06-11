---
title: "Audit rule: committed .env.production / .env.local files"
labels: ["good first issue"]
---

**Context**: `audit/config_check.rs` warns when `.env` is not gitignored, but committed `.env.production` / `.env.staging` / `.env.local` files (already tracked by git) are a stronger signal of leaked secrets.

**Files to touch**
- `crates/void-stack-core/src/audit/config_check.rs`: new `scan_committed_env_files` (use `git ls-files` via `std::process::Command`, like `vuln_patterns/config.rs::scan_git_history` does).
- Bump `rule_count()`.

**Acceptance criteria**: a fixture repo with a tracked `.env.production` yields a High finding; an untracked one does not. English message. Tests included.
