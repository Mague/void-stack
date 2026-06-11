# Contributing to Void Stack

Thanks for your interest! Void Stack is a Rust workspace with five crates:
`void-stack-core` (analysis engine), `void-stack-mcp` (MCP server),
`void-stack-cli` (`void`), `void-stack-tui`, `void-stack-desktop` (Tauri)
and `void-stack-daemon`.

## Build

```bash
git clone https://github.com/Mague/void-stack
cd void-stack
cargo build --workspace            # debug build of everything
cargo build --release -p void-stack-mcp   # the MCP server binary
```

The `vector` feature pulls fastembed (ONNX, ~130 MB model download on first
index) and `structural` pulls tree-sitter grammars. Both are default
features of the MCP/CLI crates; core builds without them.

## Test

```bash
cargo test --workspace                                   # full suite
cargo test -p void-stack-core --features vector,structural   # core with engines
void suggest-tests void-stack                            # after edits: run only what your diff touches
```

## Quality gates (CI enforces all three)

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

## PR checklist

- [ ] One logical change per PR; reference the issue.
- [ ] `cargo fmt` + clippy clean + tests green.
- [ ] New behavior has tests (fixture-based tests live next to the module).
- [ ] User-facing changes documented (README + CHANGELOG entry under `[Unreleased]`).
- [ ] Ran `void review void-stack` and addressed Critical/High findings on your changed lines.
- [ ] No new `unwrap()` in production code (the audit gates it; use `?` or a justified `expect("invariant")`).

## Good first issues

See `.github/ISSUE_DRAFTS/` for scoped starter tasks (new walker languages,
audit rules, CLI polish, docs).
