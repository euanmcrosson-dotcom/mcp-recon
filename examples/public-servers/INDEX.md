# Public-server inventories — the v0.1 evaluation dataset

These are real `mcp-recon enumerate` outputs against the official
`@modelcontextprotocol/*` servers. The week-3 classifier evaluates
against this set; the week-4 writeup uses these as the base.

| Server | npm package | Tool count | Tools |
|---|---|---|---|
| `secure-filesystem-server` | `@modelcontextprotocol/server-filesystem` | 14 | `read_file`, `read_text_file`, `read_media_file`, `read_multiple_files`, `write_file`, `edit_file`, `create_directory`, `list_directory`, `list_directory_with_sizes`, `directory_tree`, `move_file`, `search_files`, `get_file_info`, `list_allowed_directories` |
| `memory-server` | `@modelcontextprotocol/server-memory` | 9 | `create_entities`, `create_relations`, `add_observations`, `delete_entities`, `delete_observations`, `delete_relations`, `read_graph`, `search_nodes`, `open_nodes` |
| `sequential-thinking-server` | `@modelcontextprotocol/server-sequential-thinking` | 1 | `sequentialthinking` |
| `example-servers/everything` | `@modelcontextprotocol/server-everything` | 13 | `echo`, `get-annotated-message`, `get-env`, `get-resource-links`, `get-resource-reference`, `get-structured-content`, `get-sum`, `get-tiny-image`, `gzip-file-as-resource`, `toggle-simulated-logging`, `toggle-subscriber-updates`, `trigger-long-running-operation`, `simulate-research-query` |

**Totals: 4 servers, 37 tools.**

## How these were generated

Each `*.inventory.json` file is the literal stdout of:

```bash
mcp-recon enumerate "stdio:npx -y <package> <args-if-any>"
```

The schema tag is `mcp-recon/v0.1/inventory`. The `scanned_at`
field is the wall-clock time at capture; everything else is
server-controlled and reproducible.

## Regenerate

```bash
# from repo root, with the CLI built:
npx tsx packages/mcp-recon-cli/src/bin/recon.ts enumerate \
  "stdio:npx -y @modelcontextprotocol/server-memory" \
  > examples/public-servers/server-memory.inventory.json
```

## Why these four

- **filesystem** — the canonical MCP server, full read+write+delete
  surface. capnagent's `mcp-fs-agent` already integrates with it,
  so the cross-project comparison is trivial.
- **memory** — knowledge-graph CRUD. Surfaces a different shape
  (entities + relations rather than paths) so the classifier sees
  multiple data-classes.
- **sequential-thinking** — single-tool server. Stress-tests the
  classifier's lower bound (does it produce a coherent threat
  profile for N=1?).
- **everything** — Anthropic's example/test server with 13 deliberately
  diverse tools. Useful for validating the classifier handles
  unusual shapes (notification toggles, env-var reads, simulated
  research queries).

## v0.2 expansion ideas (NOT v0.1)

- Add more official servers: time, fetch, git (Python; need pip in
  the runner).
- Add community servers from the awesome-mcp-servers list.
- Add a per-server captured-at timestamp index so we can detect
  schema drift between releases.

These are all post-v0.1 — the four servers above are enough to
ship v0.1 against.
