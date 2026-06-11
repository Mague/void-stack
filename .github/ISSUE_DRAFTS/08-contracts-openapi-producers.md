---
title: "contracts.rs: OpenAPI/Swagger specs as REST producers"
labels: ["good first issue"]
---

**Context**: `vector_index/contracts.rs` extracts REST producers from code (Express/FastAPI/Next/Go). An `openapi.yaml`/`swagger.json` in the repo is a complete producer list and should beat code heuristics. `diagram/api_routes/swagger.rs` already parses these files — reuse it.

**Files to touch**: `contracts.rs` (`extract_file_contracts`: route `.yaml/.yml/.json` files whose content has an `openapi:`/`swagger:` key through a new extractor reusing `swagger.rs` parsing).

**Acceptance criteria**: a fixture openapi.yaml with two paths yields producer contracts with normalized `{param}` paths; cache invalidation still works (hash-based, free).
