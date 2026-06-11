---
title: "Add Ruby support to the structural walker"
labels: ["good first issue"]
---

**Context**: Same as the Kotlin issue but for Ruby (`tree-sitter-ruby`): `class`/`module` → Class, `def` → Function, `call` nodes → CALLS edges.

**Files to touch**: same trio as the Kotlin issue.

**Acceptance criteria**: fixture `.rb` with a class calling a helper produces nodes + a CALLS edge; workspace tests green.
