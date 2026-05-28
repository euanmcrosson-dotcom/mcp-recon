# mcp-recon

[![CI](https://github.com/euanmcrosson-dotcom/mcp-recon/actions/workflows/ci.yml/badge.svg)](https://github.com/euanmcrosson-dotcom/mcp-recon/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Part of: Capframe](https://img.shields.io/badge/part_of-Capframe-00f5a0.svg)](https://capframe.ai)

> **The Find module of [Capframe](https://capframe.ai).** A deterministic,
> rule-based classifier for MCP tool surfaces: read a tool inventory, flag the
> security-relevant patterns, emit a [`findings.v1`](https://capframe.ai/schema)
> JSON document mapped to OWASP LLM Top 10 / NIST AI RMF / MITRE ATLAS.

The thesis: every team adopting MCP right now is asking *"what does this server
actually let an agent do?"* and there's little tooling for it. mcp-recon is the
recon side of that gap. [capnagent](https://github.com/euanmcrosson-dotcom/capnagent)
(Capframe's **Bind** module) is the defensive side — take the findings, mint a
tight capability token, deny everything outside it.

> **Status:** pre-1.0, shipped as part of Capframe. The classifier is real and
> test-covered (see [Rules](#classifier-rules)). Live MCP enumeration and
> schema-aware fuzzing are **not yet shipped** — see [Roadmap](#roadmap). This
> README describes only what the binary does today.

## What it does (today)

mcp-recon takes an **inventory** of an MCP server's tools — a JSON snapshot of
each tool's name, description, parameter schema, and declared side-effects — and
runs a set of deterministic classifier rules over it. Every rule maps a
detectable signal to a severity and a bag of compliance-framework IDs. The output
is a `findings.v1` document: the same wire format Capframe's `report` module
consumes and the [schema](https://capframe.ai/schema) the whole platform is built
around.

No LLM in the loop. Same inventory in → same findings out, every run.

## Install

Via Capframe (recommended — sha256-verified):

```bash
curl -fsSL capframe.ai/install | sh
capframe install find
```

Or build from source:

```bash
cargo install --git https://github.com/euanmcrosson-dotcom/mcp-recon mcp-recon-cli
```

## Usage

Two modes — build an inventory from your real config (`enumerate`), or classify
an inventory you already have (`--target`).

```bash
# Live: point at your real claude_desktop_config.json (Cursor / Cline configs
# work too) — mcp-recon reaches each MCP server, handshakes, calls tools/list,
# and writes an inventory of the actual tools.
mcp-recon enumerate ~/Library/Application\ Support/Claude/claude_desktop_config.json \
    --out inventory.json

# Classify an inventory (hand-authored or produced by enumerate) into findings.
mcp-recon --target inventory.json --out findings.json --pretty

# Or dispatched through the Capframe umbrella CLI
capframe find ./inventory.json --out findings.json
```

`--target` takes an `mcp-recon.inventory.v1` JSON file (see [`examples/`](examples/)).
If the file can't be read or parsed, mcp-recon still emits a valid `findings.v1`
envelope with a single informational finding, so downstream tooling never sees
broken output.

`enumerate` supports **both transports**: a server entry with a `command` is
launched over **stdio** (the local case); an entry with a `url` is reached over
**HTTP** (Streamable HTTP — JSON-RPC over POST, handling both `application/json`
and `text/event-stream` replies, with `Mcp-Session-Id` carried across calls, plus
optional `headers` for auth). Per-server handshake has a 15s default timeout
(`--timeout-secs`); a server that fails to connect becomes an empty inventory
entry rather than aborting the run.

```jsonc
{
  "mcpServers": {
    "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
    "remote":     { "url": "https://mcp.example.com/mcp", "headers": { "Authorization": "Bearer …" } }
  }
}
```

## Classify non-MCP tool surfaces (`mcp-recon adapt`)

You don't have to be on MCP for mcp-recon to be useful. If your agent already
describes its tools in Anthropic's `tool_use` format or OpenAI's
function-calling format, `mcp-recon adapt` converts that payload into an
`mcp-recon.inventory.v1` document — same downstream pipeline, same findings,
same caveats artifact.

```bash
mcp-recon adapt --format anthropic ./examples/anthropic-tools.json \
    --out inventory.json
mcp-recon adapt --format openai    ./examples/openai-tools.json    \
    --out inventory.json
mcp-recon --target inventory.json --out findings.json --pretty
```

Both formats accept either a bare array of tool entries or a `{ "tools": [...] }`
wrapper. OpenAI's current `{ type: "function", function: {...} }` shape and
the deprecated bare `{ name, description, parameters }` form both work; you
can even mix them in the same file. `--server-name` overrides the default
inventory server name (which is otherwise derived from the input filename's
stem). See [`examples/anthropic-tools.json`](examples/anthropic-tools.json)
and [`examples/openai-tools.json`](examples/openai-tools.json) for the input
shapes the adapter understands.

What the adapter cannot infer — `side_effects`, `auth_required`, and
`rate_limited` are not declared in either provider's tool format — is left
empty / unset. The classifier still surfaces authority signals via R3 (name
implies mutation), R5 (description mentions money), R6 (description implies
external fetch), and R7 (code execution) without needing those declarations.

## Run mcp-recon *as* an MCP server (`mcp-recon mcp-server`)

mcp-recon also speaks the protocol it scans. `mcp-recon mcp-server` turns the
binary into a stdio MCP server that any MCP-aware agent (Claude Desktop, Cursor,
your own framework) can connect to. It exposes two tools — `classify_inventory`
and `caveats` — backed by the same deterministic core the CLI runs.

```jsonc
// claude_desktop_config.json (or equivalent)
{
  "mcpServers": {
    "mcp-recon": { "command": "mcp-recon", "args": ["mcp-server"] }
  }
}
```

Each tool takes an `inventory` argument shaped per
[`mcp-recon.inventory.v1`](schemas/) and returns its result as a JSON-encoded
text content block. The server speaks newline-delimited JSON-RPC 2.0 per the
MCP 2025-03-26 spec; no live enumeration in this mode — supply a pre-built
inventory.

## Classifier rules

Seven deterministic rules today. Each is a small function with unit tests plus
integration tests against the committed example inventories.

| Rule | Fires when | Severity | OWASP / NIST / ATLAS |
|---|---|---|---|
| **R1** | A string parameter has no `maxLength` | medium | LLM01 / MEASURE-2.3 / T0051 |
| **R2** | A tool declares side-effects but no `auth_required` | high | LLM07 / MANAGE-1.3 / T0049 |
| **R3** | Tool name implies a mutation not in declared `side_effects` | high | LLM08 / MEASURE-2.6 / T0051 |
| **R4** | A money/quota-named numeric param has no `maximum` | high | LLM08 / MANAGE-2.2 / T0051 |
| **R5** | Description mentions money but no `money` side-effect declared | medium | LLM08 / MEASURE-2.6 / T0040 |
| **R6** | Description implies fetching external web content | medium | LLM01 / MEASURE-2.3 / T0051 |
| **R7** | Name/description implies code or command execution | **critical** | LLM08 / MANAGE-2.2 / T0051 |

Adding a rule is ~40 lines + tests; see
[`crates/mcp-recon-core/src/classifier.rs`](crates/mcp-recon-core/src/classifier.rs)
for the pattern, and the open issues for `good first issue`-tagged candidates
(e.g. command-injection-via-shell-arg detection, an "undeclared side-effects"
rule).

## Example: the Damn Vulnerable MCP Server

[`examples/dvmcp.inventory.json`](examples/dvmcp.inventory.json) is a faithful
inventory of four challenges from the
[Damn Vulnerable MCP Server](https://github.com/harishsg993010/damn-vulnerable-MCP-server).

```bash
mcp-recon --target examples/dvmcp.inventory.json --out findings.json --pretty
```

Produces **12 findings — 2 critical** (`execute_python_code`,
`execute_shell_command` via R7) + 10 medium (unconstrained inputs via R1). Full
writeup, including the rule gap this scan originally exposed:
[capframe.ai/blog/scanning-the-damn-vulnerable-mcp-server](https://capframe.ai/blog/scanning-the-damn-vulnerable-mcp-server).

Two more example inventories ship in [`examples/`](examples/):
`shopify-mcp.inventory.json` (6 findings) and `safe-mcp.inventory.json` (0 —
what a well-declared tool surface looks like).

## From finding to enforcement (`mcp-recon caveats`)

Findings tell you what's wrong; `caveats` tells you what to *do* about it. It
classifies an inventory and emits `mcp-recon/v0.1/caveats` — a capnagent-ready
issuance plan per authority-relevant tool:

```bash
mcp-recon caveats inventory.json --out caveats.json --pretty
```

```jsonc
{
  "schema": "mcp-recon/v0.1/caveats",
  "plans": [
    { "tool": "puppeteer_evaluate", "recommend": "deny",
      "caveats": ["tool != \"puppeteer_evaluate\""], "provenance": ["r1","r7"],
      "note": "Code/command-execution surface (R7)… do not grant this tool." },
    { "tool": "order.refund", "recommend": "scope",
      "caveats": ["tool == \"order.refund\"", "arg.amount <= 100"],
      "provenance": ["r4"], "note": "…the `<= 100` limit is a placeholder." }
  ]
}
```

R7 code-execution tools become **`deny`** plans; everything else becomes a
**`scope`** plan (`tool == "…"`, plus an `arg.<param> <= …` cap for unbounded
money/quota numerics). Feed the artifact straight into your
[capnagent](https://github.com/euanmcrosson-dotcom/capnagent) issuer.

## How it fits Capframe

```
inventory.json ─▶ mcp-recon (Find) ─▶ findings.v1.json ─▶ capframe report
                       │
                       ▼  mcp-recon caveats
              mcp-recon/v0.1/caveats ─▶ capnagent (Bind) — issue a scoped token
```

mcp-recon classifies the surface and emits both findings (`capframe report`
renders them to HTML/PDF mapped to the compliance frameworks) and a caveats
artifact that capnagent turns into a capability token.

## Roadmap — not yet shipped

These are the ambitions, stated honestly as not-yet-built:

- **WebSocket transport.** Stdio and HTTP (Streamable HTTP) enumeration ship
  today (`mcp-recon enumerate`); a WebSocket transport is the remaining one.
- **Schema-aware fuzzing.** Generate adversarial inputs against each tool's
  parameter schema to surface runtime defects (DoS, deserialization).
- **Markdown threat-profile rendering.** A human-readable report companion to
  the JSON.

The core crate has stubbed module hooks for some of these; none are wired into
the shipped CLI yet. PRs welcome.

## What this is NOT

- It does **not** *call* your tools. `enumerate` connects (stdio/HTTP),
  handshakes, and reads `tools/list` — it never invokes a tool, so enumeration
  has no side effects. Classification is then a separate, offline, deterministic
  pass over that inventory.
- It is **not** an LLM-based tool. Rules are deterministic and auditable.
- It does **not** prove a tool is exploitable — it flags patterns worth a human's
  attention, mapped to known threat classes. A heuristic classifier is a floor,
  not a ceiling.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

Apache-2.0. See [LICENSE](LICENSE).
