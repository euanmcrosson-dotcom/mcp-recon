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

- **`example-servers/everything` has a DoS-shaped surface on
  `trigger-long-running-operation`.** Fuzzing with `steps:
  9007199254740992` (Number.MAX_SAFE_INTEGER + 1) caused the call
  to time out at 5s — the server faithfully attempts the requested
  iteration count rather than rejecting it as out-of-range. Not a
  validation crash; a missing upper-bound check that lets a
  prompt-injected agent burn CPU. Recorded in
  `server-everything.fuzz.json` as the only runtime_error in the
  dataset (the other 3 servers produced 0).
- **filesystem is the most strict** at 98% protocol_error rate.
  This matches its production role.
- **sequential-thinking accepts ~60% of inputs.** Single tool with
  one large opaque schema; fuzzer's argument-typed assumptions
  don't help here. Useful contrast for the writeup.
- **memory's `delete_*` tools are NOT flagged confused-deputy** by
  the v0.1 classifier — they take object/array args (entity-name
  lists), not free-form strings. This is the classifier behaving
  correctly on a non-string-arg destructive tool: capnagent's
  threat model differs.

These observations come from running both the fuzzer (week 2) and
the classifier (week 3) against each server. The full
`*.classification.json` and `*.report.md` for each are committed
alongside the inventories and fuzz outputs.

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
