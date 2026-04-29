# Public-server inventories + fuzz — the v0.1 evaluation dataset

These are real `mcp-recon enumerate` and `mcp-recon fuzz` outputs
against the official `@modelcontextprotocol/*` servers. The week-3
classifier evaluates against this set; the week-4 writeup uses
these as the base.

| Server | npm package | Tools | Fuzz calls | ok | protocol_error | runtime_error |
|---|---|---|---|---|---|---|
| `secure-filesystem-server` | `@modelcontextprotocol/server-filesystem` | 14 | 199 | 4 | 195 | 0 |
| `memory-server` | `@modelcontextprotocol/server-memory` | 9 | 124 | 18 | 106 | 0 |
| `sequential-thinking-server` | `@modelcontextprotocol/server-sequential-thinking` | 1 | 15 | 9 | 6 | 0 |
| `example-servers/everything` | `@modelcontextprotocol/server-everything` | 13 | 151 | 51 | 99 | **1** |

**Totals: 4 servers, 37 tools, 489 fuzz calls.** Fuzz budget: 15
calls/tool. Default seed (`0xC0FFEE`).

## Notable findings (preview for the v0.1 writeup)

- **`example-servers/everything` produced 1 runtime_error.** Of the
  4 servers tested, only this one had a fuzz input that escaped the
  protocol-error layer and produced an unhandled runtime error. The
  exact input is recorded in `server-everything.fuzz.json`. Since
  this is Anthropic's public *example* server, the finding is "even
  the reference implementations have at least one fuzz-resistant
  validation gap" — a useful framing for the writeup.
- **filesystem is the most strict** at 98% protocol_error rate.
  This matches its production role.
- **sequential-thinking accepts ~60% of inputs.** Single tool with
  one large opaque schema; fuzzer's argument-typed assumptions
  don't help here. Useful contrast.

These observations are pre-classifier; the week-3 classifier will
turn raw counts into structured authority claims.

## How these were generated

Inventory:
```bash
mcp-recon enumerate "stdio:npx -y <package> <args-if-any>" \
  > server-X.inventory.json
```

Fuzz (budget 15 to keep the artefacts small; production runs use 200):
```bash
mcp-recon fuzz "stdio:npx -y <package> <args-if-any>" --budget=15 \
  > server-X.fuzz.json
```

Schema tags: `mcp-recon/v0.1/inventory` and `mcp-recon/v0.1/fuzz`.
The `scanned_at` field is wall-clock; everything else is
server-controlled or seeded-PRNG-controlled and reproducible.

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
