---
title: "dead-code: Swift visibility heuristic"
labels: ["good first issue"]
---

**Context**: `crates/void-stack-core/src/deadcode.rs` labels candidates high/medium by language-specific visibility (Rust `pub`, Go capitalization, Dart `_`). Swift files fall into the conservative `_ => true` arm, so everything is `medium`.

**Files to touch**: `deadcode.rs` — Swift: `private`/`fileprivate` on the declaration line → not exported (high confidence when zero callers); `public`/`open` → medium.

**Acceptance criteria**: unit test with a Swift declaration-line table; caveats note updated.
