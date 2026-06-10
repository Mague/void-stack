---
name: skill-void-stack
description: Working rules for coding sessions on projects managed by void-stack (MCP). Use whenever implementing, testing, or committing in a registered project — covers the short test loop (suggest_tests_for_diff) and the pre-commit review loop (review_diff).
---

# void-stack session workflow

Standing rules for any coding session on a void-stack-registered project.

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
4. The payload is compact (≤4k tokens) by design — paste it into the
   reviewer context instead of raw diffs when asking for an LLM review.

## Supporting tools

- `build_structural_graph` first if either tool reports a missing graph.
- `get_impact_radius` for a wider blast-radius view than review_diff's
  depth-2 summary.
- `audit_project` for the full project audit (review_diff only scans the
  changed lines).
