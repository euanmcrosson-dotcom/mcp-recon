# Public-server inventories + fuzz — the v0.1 evaluation dataset

These are real `mcp-recon scan` outputs against publicly-published
reference and example MCP servers. Every artefact is reproducible
from a fresh checkout via the regenerate commands at the bottom of
this file.

| Server | Source | Tools | Fuzz calls | ok | protocol_error | runtime_error | CDP |
|---|---|---|---|---|---|---|---|
| `secure-filesystem-server` | `@modelcontextprotocol/server-filesystem` | 14 | 723 | 4 | 719 | 0 | 4 |
| `memory-server` | `@modelcontextprotocol/server-memory` | 9 | 146 | 29 | 117 | 0 | 0 |
| `sequential-thinking-server` | `@modelcontextprotocol/server-sequential-thinking` | 1 | 134 | 22 | 112 | 0 | 0 |
| `example-servers/everything` | `@modelcontextprotocol/server-everything` | 13 | 371 | 86 | 278 | **7** | 1 |
| `example-servers/puppeteer` | `@modelcontextprotocol/server-puppeteer` | 7 | 357 | 64 | 187 | **106** | 5 |
| `mcp-git` | `mcp-server-git` (PyPI) | 12 | 792 | 0 | 792 | 0 | 0 |
| `mcp-fetch` | `mcp-server-fetch` (PyPI) | 1 | 78 | 0 | 78 | 0 | 0 |
| `mcp-time` | `mcp-server-time` (PyPI) | 2 | 132 | 0 | 132 | 0 | 0 |
| `example-servers/postgres` | `@modelcontextprotocol/server-postgres` | 1 | 28 | 0 | 28 | 0 | 1 |

**Totals: 9 servers, 60 tools, 2761 fuzz calls, 113 runtime_errors,
11 confused-deputy candidates.** Default fuzz budget (200
calls/tool). Default seed (`0xC0FFEE`). Note: per-tool generated
inputs cap at min(generators, budget); not every tool has 200 axes
of input shapes, so totals are smaller than 60 × 200.

## Notable findings

The full writeups live in [`/findings/`](../../findings/). Headlines:

- **`example-servers/puppeteer` has a 5s-per-call DoS amplification on
  every selector-taking tool.** At budget=200 the fuzzer triggers 106
  `timeout after 5000ms` runtime_errors across `puppeteer_fill`,
  `puppeteer_select`, `puppeteer_hover`, and one each on
  `puppeteer_click`/`puppeteer_screenshot`. The server uses Puppeteer's
  default 30s `waitForSelector` (clipped at the MCP transport's 5s
  limit) and offers no timeout argument. A prompt-injected agent can
  burn 5s of server time per call by feeding non-existent selectors;
  with concurrent calls this trivially saturates the worker. See
  [`F002`](../../findings/F002-server-puppeteer-selector-timeout-amplification.md).
- **`example-servers/puppeteer.puppeteer_evaluate` is literal arbitrary
  code execution in the browser sandbox.** The classifier flags it
  `shell/privileged` with a `tool != "puppeteer_evaluate"` deny-by-default
  caveat. Not a vulnerability in the upstream — the tool is documented as
  "Execute JavaScript in the browser console" — but operators wiring
  this server into an agent should know it bypasses every other URL
  allowlist they configure on `puppeteer_navigate`.
  See [`F003`](../../findings/F003-server-puppeteer-evaluate-privileged.md).
- **`example-servers/postgres.query` is described as "read-only" but
  the schema permits arbitrary SQL.** The classifier escalates to
  `write/confused-deputy` because there's no schema-level constraint
  enforcing the description's "read-only" promise — it's enforced
  inside the implementation (a `BEGIN; ROLLBACK;` block). An operator
  can't prove the read-only property from the recon output alone, so
  capnagent should bound `arg.sql starts_with "SELECT "` even though
  the upstream advertises read-only. See [`F004`](../../findings/F004-server-postgres-readonly-claim-undocumented-in-schema.md).
- **`example-servers/everything` has a reliable DoS-shaped surface
  on `trigger-long-running-operation`.** At budget=200 the
  boundary-axis fuzzer hits 7 separate timeout events. Reproducible
  missing upper-bound check that lets a prompt-injected agent burn
  server CPU on demand. See [`F001`](../../findings/F001-server-everything-doslogging-amplification.md).
- **`mcp-git` enforces a strict repository allowlist via the
  `--repository` CLI flag.** 210 of 792 fuzz calls return
  `Repository path 'X' is outside the allowed repository '<sandbox>'`
  — every path-walking tool checks the boundary. A positive
  defence-in-depth example mcp-recon should highlight, contrasted
  against the looser `server-filesystem`. See [`F005`](../../findings/F005-server-git-repository-allowlist-positive-control.md).
- **mcp-recon classifier mis-tags timezone strings as filesystem
  paths.** `mcp-time.convert_time` arguments `source_timezone` and
  `target_timezone` accept IANA names like `America/New_York`. The
  classifier's "path-shaped" heuristic fires on the slash and
  produces a `data_class=filesystem` rationale that's plainly wrong.
  Recon-side finding (mcp-recon bug). See [`F006`](../../findings/F006-classifier-timezone-false-positive.md).

## How these were generated

```bash
mcp-recon scan "stdio:<command>" \
  --out=examples/public-servers/server-X --budget=200
```

The `scan` orchestrator runs enumerate → fuzz → classify → report
in one shot, writing four files per server:

```
server-X/
  inventory.json        mcp-recon/v0.1/inventory
  fuzz.json             mcp-recon/v0.1/fuzz
  classification.json   mcp-recon/v0.1/classification
  report.md             Markdown threat profile (human-readable)
```

The `scanned_at` fields are wall-clock; everything else is
server-controlled or seeded-PRNG-controlled and reproducible.

## Why these nine

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
- **puppeteer** *(NEW)* — browser automation. Adds the `shell/privileged`
  data class via `puppeteer_evaluate` plus 4 confused-deputy
  candidates on selector-taking tools. Surfaces the headline DoS
  finding (F002) and the privileged-deny finding (F003).
- **git** *(NEW)* — version control. 12-tool surface, every tool takes
  a `repo_path` argument. Tests the classifier on a server that
  enforces a strict allowlist via CLI flag (a positive control —
  F005).
- **fetch** *(NEW)* — HTTP fetcher. Single-tool server but with rich
  SSRF surface (`url`, `max_length`, `start_index`, `raw`). Tests the
  classifier's URL-shaped argument detection. Pydantic-strict input
  schema rejects every fuzz axis cleanly.
- **time** *(NEW)* — timezone conversion. Smallest surface in the
  dataset (2 tools). Triggers a classifier false positive (F006) on
  `convert_time` because IANA timezone names contain slashes.
- **postgres** *(NEW, deprecated)* — single-tool SQL server. The
  upstream is npm-deprecated but still installable. The `query` tool's
  description claims "read-only" but the schema doesn't constrain it —
  the recon output can't verify the read-only invariant, so
  capnagent must bound it externally (F004).

## What's still missing (v0.2 candidates)

- **HTTP-transport servers.** `@modelcontextprotocol/server-pdf`,
  `server-map`, `server-threejs`, and `server-transcript` all run on
  Streamable HTTP and are not stdio-spawnable. v0.1's transport layer
  is stdio-only.
- **Credentialled servers.** `server-slack`, `server-github`,
  `server-gdrive`, `server-brave-search`, `server-google-maps` all
  refuse to start without API keys. The MCP SDK's
  `StdioClientTransport` whitelists a small set of env vars (PATH,
  HOME, etc.) on Windows — `SLACK_BOT_TOKEN` and friends are
  explicitly stripped. Fixing this requires either (a) a
  passthrough-env CLI flag or (b) running the server with the env
  pre-baked into the spawn command. Both are v0.2.
- **`server-redis`** — needs a live Redis instance; v0.1's "no
  side-effects against a third party" rule disqualifies it.

## Regenerate

```bash
# from repo root, with deps installed via `npm ci`:

# Existing 4 (npm packages)
npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:npx -y @modelcontextprotocol/server-filesystem /c/tmp/mcp-recon-sandbox-filesystem" \
  --out=examples/public-servers/server-filesystem --budget=200

npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:npx -y @modelcontextprotocol/server-memory" \
  --out=examples/public-servers/server-memory --budget=200

npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:npx -y @modelcontextprotocol/server-sequential-thinking" \
  --out=examples/public-servers/server-sequential-thinking --budget=200

npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:npx -y @modelcontextprotocol/server-everything" \
  --out=examples/public-servers/server-everything --budget=200

# 5 new servers (npm + pip)
npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:npx -y @modelcontextprotocol/server-puppeteer" \
  --out=examples/public-servers/server-puppeteer --budget=200

# pip install mcp-server-git mcp-server-fetch mcp-server-time
npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:python -m mcp_server_git --repository C:/tmp/mcp-recon-sandbox-git" \
  --out=examples/public-servers/server-git --budget=200

npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:python -m mcp_server_fetch" \
  --out=examples/public-servers/server-fetch --budget=200

npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:python -m mcp_server_time" \
  --out=examples/public-servers/server-time --budget=200

npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:npx -y @modelcontextprotocol/server-postgres postgresql://invalid:invalid@localhost:5432/invalid" \
  --out=examples/public-servers/server-postgres --budget=200
```

The `server-postgres` scan uses a deliberately bogus connection
string. The server enumerates its tool surface before opening the
connection, so we still see the (single) `query` tool in the
inventory, and the fuzzer's protocol_error count just reflects
"server can't reach DB" — which is the safe failure mode for an
unauthenticated audit.
