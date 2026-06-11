---
title: "Audit rule: JWT 'none' algorithm and weak HS256 secrets"
labels: ["good first issue"]
---

**Context**: `audit/vuln_patterns/crypto.rs` covers weak hashing. JWT misconfig is a classic: `algorithm: 'none'` / `alg: "none"`, and `jwt.sign(payload, 'secret')` with short literals.

**Files to touch**: `crates/void-stack-core/src/audit/vuln_patterns/crypto.rs` (+ `rule_count()` in `vuln_patterns/mod.rs`).

**Acceptance criteria**: fixture lines in JS/TS/Python flagged (Critical for `none`, High for short literal secret); comments not flagged; tests included.
