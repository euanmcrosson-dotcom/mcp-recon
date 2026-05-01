# mcp-recon v0.1 — specification + non-goals

This document is **load-bearing**. The goal is a v0.1 release in 4
weeks. Anything not listed below is out of scope for v0.1, even if
"obviously useful." Scope creep is the project's biggest risk.

## What v0.1 does

```
mcp-recon enumerate <server-spec>
  → connects to the MCP server (stdio or http)
  → lists tools (name, description, inputSchema)
  → emits one JSON document with the full tool inventory

mcp-recon fuzz <server-spec> [--budget=N]
  → runs schema-aware adversarial inputs against each tool
  → records: schema-violations, encoding tricks, type-confusion,
    boundary values, missing required fields, extra fields
  → emits one JSON document with per-tool fuzz results

mcp-recon classify <inventory.json>
  → applies the OWASP LLM Top 10 + MITRE ATLAS classifier ruleset
  → tags each tool with: data-class (filesystem / network / shell /
    payments / messaging / system / metadata), authority-level,
    confused-deputy potential
  → emits one JSON document with classifications

mcp-recon report <inventory.json> <fuzz.json> <classification.json>
  → emits a Markdown threat profile per server
  → includes the recommended capnagent caveat per tool

mcp-recon caveats <classification.json>
                  [--caller=ID] [--sandbox-prefix=PATH] [--expiry=ISO]
  → ingests a classification, applies operator-supplied bindings to
    placeholder tokens (<your-caller-id>, <your-sandbox-prefix>,
    <your-cap-expiry>), splits AND-joined predicates into individual
    capnagent DSL caveats, flags plans that need review (unknown
    classification, low confidence, confused-deputy without arg
    constraint, unsubstituted placeholder)
  → emits one JSON document (schema mcp-recon/v0.1/caveats) suitable
    for direct ingestion by a capnagent issuer — one element per
    Issuer.caveat(...) call

mcp-recon scan <server-spec>
  → runs enumerate + fuzz + classify + report in one shot
  → the daily-driver command
```

That's the entire surface for v0.1. Six commands.

## Server-spec syntax

Three forms:

```
stdio:npx @modelcontextprotocol/server-filesystem /tmp
stdio:./path/to/binary --arg
http://localhost:3000
```

The `stdio:` prefix means "spawn this process and talk over stdin/
stdout per the MCP stdio transport." The `http://` form uses MCP's
HTTP transport. We support both because the MCP ecosystem is split
across them.

## Output format — one JSON document per command

Every command writes a self-describing JSON document to stdout:

```json
{
  "schema": "mcp-recon/v0.1/inventory",
  "scanned_at": "2026-04-29T12:34:56Z",
  "server": { ... },
  "tools": [ { ... }, ... ]
}
```

The `schema` field is the load-bearing contract. Downstream tools
(including capnagent's caveat-suggestion bridge) parse based on
this string.

## Classification rules — keep it simple

The v0.1 classifier is **rule-based, not ML-based**. Heuristics:

1. **Tool description** is matched against a curated set of regex
   keywords per data-class. (e.g. `read_file|list_directory|`
   `directory_tree` → filesystem; `fetch|get|post|http` → network;
   `exec|shell|run|spawn` → shell.)
2. **`inputSchema`** is inspected for argument types that suggest
   ambient authority (path-shaped strings, URL-shaped strings,
   command-shaped arrays).
3. **Cross-product flag**: if a tool has user-controllable string
   args AND a side-effecting verb in its description, it's a
   **confused-deputy candidate**.

Full rule table: `crates/mcp-recon-core/src/rules/v0_1.rs`.
Heuristic and explicit; v0.2 may add an ML-based classifier as a
secondary signal but v0.1 is strictly rules.

## Fuzzing strategy — schema-aware, not random

For each tool, the fuzzer generates inputs from the JSON Schema
across these axes:

- **Boundary values** — empty string, max-length string, 0, -1,
  Number.MAX_SAFE_INTEGER + 1, NaN, null
- **Type confusion** — string when number expected, array when
  object expected, etc.
- **Encoding tricks** — percent-encoding, Unicode homographs,
  null bytes, backslash escapes
- **Path traversal** — for path-shaped args: `../`, `%2e%2e/`,
  Windows `\\`-separators on POSIX, etc.
- **URL hostility** — for URL-shaped args: userinfo splitting,
  IDN homograph, scheme tricks, port tricks
- **Schema-violation** — extra fields, missing required fields,
  wrong types in nested structures

Default budget: 200 calls per tool. Configurable via `--budget`.
Outputs are stdout strings; we **do not** characterize whether a
particular fuzz "succeeded" — the classifier and human review do
that. v0.1 records what happened; v0.2 may add automated outcome
detection.

## What v0.1 does NOT do (explicit non-goals)

- **No replay attacks.** That's a separate tool category (Burp /
  ZAP for MCP).
- **No LLM-in-the-loop.** v0.1 is deterministic; you can run it
  100x and get bit-identical results. ML-based classification is
  v0.2.
- **No active exploitation.** We send adversarial schemas, not
  actual attacks. We don't try to write `~/.ssh/id_rsa`. (The
  caveat-evaluation gate this exposes is what capnagent fixes.)
- **No proxy mode.** v0.1 is a recon tool, not a man-in-the-middle.
- **No multi-server cross-product fuzzing.** That's the round-01
  cross-server confused-deputy class — interesting but out of
  scope for v0.1.
- **No GUI / dashboard.** The scan output is JSON + Markdown; if
  someone wants a dashboard they can build one on top.
- **No persistence layer.** Reports go to stdout / files; we don't
  ship a DB.
- **No authentication shimming.** Servers requiring auth are out
  of scope for v0.1; the user is responsible for providing a
  pre-authenticated transport.

## Performance targets

- **Enumerate:** finishes in < 5 seconds against any reasonable
  server.
- **Fuzz with default budget:** finishes in < 60 seconds for a
  10-tool server.
- **Classify + report:** O(tools), < 1 second.
- **Memory:** under 256 MB even on a 100-tool server.

## Determinism

The fuzz step uses a seeded PRNG (default seed: `0xC0FFEE`).
Re-running with the same seed against the same server gives
bit-identical fuzz inputs. This is the reproducibility contract
for any finding.

## Test target — `@modelcontextprotocol/server-filesystem`

We pin our v0.1 evaluation against the official filesystem MCP
server because:

1. It's stable, documented, and widely deployed.
2. It exposes a tool surface (read_file, list_directory,
   directory_tree, write_file, create_directory, delete_path) that
   covers all the data-classes in our classifier.
3. capnagent already integrates with it — using the same target
   keeps the two projects's evaluation surfaces aligned.

## Week-by-week milestones (4-week cap)

| Week | Deliverable |
|------|-------------|
| 1 | `mcp-recon enumerate <stdio:>` produces a valid inventory JSON for the filesystem server |
| 2 | `mcp-recon fuzz` runs the schema-aware fuzzer with 200-call budget |
| 3 | `mcp-recon classify` + `mcp-recon report` produce a Markdown threat profile |
| 4 | `mcp-recon scan` against ≥ 10 public MCP servers; writeup published |

If a week's milestone slips, **cut scope, don't extend the
schedule.** The v0.1 ship date is the end of week 4 regardless.

## v0.2 backlog (NOT v0.1)

Listed only so reviewers can see what's coming, NOT to commit:

- ML-based classification as secondary signal
- HTTP transport beyond stdio (currently stdio-only in v0.1)
- Multi-server cross-product fuzzing (round-01 shape)
- Replay / proxy mode
- Persistence layer for longitudinal scanning
- Authenticated-transport shims
- A small web UI for browsing reports

These all turn into separate decisions at the end of v0.1, after
the writeup lands and we see what reviewers actually want.
