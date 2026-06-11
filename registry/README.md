# MCP directory submissions

Metadata for listing void-stack-mcp in MCP directories. **Every one of
these requires a manual step** — schemas drift, verify each format
against the directory's current docs right before submitting:

| Directory | File | Submission path | Manual steps |
|---|---|---|---|
| Official MCP Registry (registry.modelcontextprotocol.io) | `server.json` | `mcp-publisher publish` CLI | GitHub auth via the publisher CLI; namespace `io.github.mague/*` must match the repo owner. |
| Smithery | `smithery.yaml` | Place at REPO ROOT, then claim the server at smithery.ai | Web login + repo claim. Binary servers may need their Docker path — verify schema. |
| Glama | `glama.json` | Place at repo root as `glama.json` | Glama crawls GitHub; claim the listing at glama.ai to edit metadata. |

Also worth listing manually (no metadata file needed):
- `modelcontextprotocol/servers` community README (PR adding a row)
- Cursor Directory, PulseMCP, mcp.so (web forms)
