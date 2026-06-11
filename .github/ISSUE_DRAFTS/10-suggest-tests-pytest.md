---
title: "suggest-tests: pytest runner commands"
labels: ["good first issue"]
---

**Context**: `crates/void-stack-core/src/testing.rs::runner_commands` emits cargo/go/flutter/jest commands. Python tests get nothing.

**Files to touch**: `testing.rs` — `"python"` arm: `pytest <file>::<test_name>` (node ids use `::`), label best-effort.

**Acceptance criteria**: unit test in `test_runner_commands_per_language` covering a python test node; README workflow section mentions pytest.
