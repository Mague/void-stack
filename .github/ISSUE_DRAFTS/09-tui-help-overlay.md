---
title: "TUI: keyboard shortcut help overlay"
labels: ["good first issue"]
---

**Context**: `void-stack-tui` has shortcuts (tabs, start/stop, logs) with no in-app discovery.

**Files to touch**: `crates/void-stack-tui/src/ui/` — render an overlay on `?` listing key bindings; `app.rs` for the toggle state.

**Acceptance criteria**: pressing `?` toggles the overlay; ESC closes; bindings listed match `app.rs` handlers. i18n strings added to `i18n.rs` (EN + ES).
