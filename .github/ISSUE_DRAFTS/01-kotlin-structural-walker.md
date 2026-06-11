---
title: "Add Kotlin support to the structural walker"
labels: ["good first issue"]
---

**Context**: The structural graph (`crates/void-stack-core/src/structural/`) parses Rust, Python, JS/TS, Go, Dart, Java, PHP, C/C++ and Elixir via tree-sitter. Kotlin projects get no call graph.

**Files to touch**
- `Cargo.toml` (workspace): add `tree-sitter-kotlin`.
- `crates/void-stack-core/src/structural/langs/mod.rs`: extension + grammar + walker mapping.
- `crates/void-stack-core/src/structural/langs/others.rs`: add `OtherLang::Kotlin` with `is_class_node` / `is_function_node` / `is_call_node` / `is_import_node` predicates (inspect the grammar's node kinds with `tree.root_node().to_sexp()`).

**Acceptance criteria**
- A fixture `.kt` file with a class, two methods and a cross-method call produces Class/Function nodes and a CALLS edge (test in `parser.rs`, mirror `test_parse_dart_class_and_method`).
- `cargo test -p void-stack-core --features structural` green.
