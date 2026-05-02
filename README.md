# mcp-recon

[![CI](https://github.com/euanmcrosson-dotcom/mcp-recon/actions/workflows/ci.yml/badge.svg)](https://github.com/euanmcrosson-dotcom/mcp-recon/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Status: pre-alpha](https://img.shields.io/badge/status-pre--alpha-orange.svg)](docs/SPEC.md)
[![Tests: 68 passing](https://img.shields.io/badge/tests-68_passing-success.svg)](#tests)
[![Companion: capnagent](https://img.shields.io/badge/companion-capnagent-9cf.svg)](https://github.com/euanmcrosson-dotcom/capnagent)

> **Reverse-engineer any MCP server's tool surface in 30 seconds.**
> Connects to an MCP server (stdio or HTTP), enumerates its tools,
> runs a schema-aware adversarial fuzzer, classifies the authority
> each tool exposes against OWASP LLM Top 10 and MITRE ATLAS, and
> emits a structured threat profile — JSON for machines, Markdown
> for humans.

The thesis: every team adopting MCP right now is asking *"what
does this server actually do?"* and there's no tooling for it. The
agentic ecosystem grew faster than its security tooling. mcp-recon
is the recon side of that gap. [capnagent](https://github.com/euanmcrosson-dotcom/capnagent)
is the defensive side: take a recon report, derive a tight
capability caveat, deny everything outside it.

> **Status:** v0.1.2 shipped 2026-04-30. Public dataset of every
> stdio TypeScript MCP server in Anthropic's `@modelcontextprotocol/*`
> namespace audited. See [`docs/WRITEUP.md`](docs/WRITEUP.md) for the
> headline findings (DoS surface on `everything`,
> missing-bounds finding on `filesystem` example wrapper, full
> server-maturity ranking).

## Contents

- [At a glance](#at-a-glance)
- [What you get](#what-you-get)
- [Command cheatsheet](#command-cheatsheet)
- [Sample output](#sample-output)
- [Recon → capnagent in one pipe](#recon--capnagent-in-one-pipe)
- [Why this exists](#why-this-exists)
- [Installation](#installation)
- [How it compares](#how-it-compares)
- [What this is NOT](#what-this-is-not)
- [Tests](#tests)
- [Companion project — capnagent](#companion-project--capnagent)
- [License](#license)

## At a glance

| Coverage | Surface | Performance |
|---|---|---|
| **4 / 4** Anthropic reference servers scanned | **5** commands · **4** schema-tagged artefacts | scan budget=200 in <60s on 14-tool server |
| **37 tools classified** across the public dataset | enumerate · fuzz · classify · report · scan | deterministic (seeded PRNG, default `0xC0FFEE`) |
| **1374 fuzz calls** across the dataset (1 confirmed DoS finding) | rules-based, not LLM-mediated | <256MB memory on 100-tool server |

Maps tools to **OWASP LLM01 / LLM06 / LLM08** and **MITRE ATLAS** categories. Every output ships with a copy-pasteable [capnagent](https://github.com/euanmcrosson-dotcom/capnagent) caveat per tool. Reproducibility contract in
[capnagent's `docs/EVALUATION.md`](https://github.com/euanmcrosson-dotcom/capnagent/blob/master/docs/EVALUATION.md).

## What you get

Run `mcp-recon scan` against any MCP server (stdio or HTTP) and get a
folder of evidence: a tool inventory, a fuzz transcript, a
classification, and a Markdown threat profile that a security reviewer
or developer-on-call can actually read. The JSON files are the
machine-parseable evidence the writeup links to. Run against any of
the 4 servers in the public dataset and your output matches
`examples/public-servers/server-<name>/` byte-for-byte.

## Command cheatsheet

```bash
mcp-recon enumerate <server-spec>                                # → inventory.json
mcp-recon fuzz      <server-spec> [--budget=N] [--seed=N]        # → fuzz.json
mcp-recon classify  <inventory.json> [--fuzz=<fuzz.json>]        # → classification.json
mcp-recon report    <inventory.json> <classification.json> [--fuzz=<fuzz.json>]  # → report.md
mcp-recon scan      <server-spec> --out=<dir> [--budget=N] [--seed=N]            # → 4 artefacts
```

Server-spec forms: `stdio:<command> [args...]` (spawn process, talk
over stdio) or `http://host:port` (HTTP transport).

## Sample output

```bash
$ mcp-recon scan "stdio:npx -y @modelcontextprotocol/server-filesystem /tmp" \
    --out=./reports/filesystem --budget=200

mcp-recon: 14 tools, 4 confused-deputy candidates
mcp-recon: fuzz — ok=4 protocol_error=719 runtime_error=0
mcp-recon: wrote 4 artefacts to ./reports/filesystem/

$ ls ./reports/filesystem/
inventory.json   fuzz.json   classification.json   report.md
```

A snippet from the resulting `classification.json` — every tool gets
a class, an authority level, a confused-deputy verdict, and a
copy-pasteable capnagent caveat:

```json
{
  "tool": "edit_file",
  "data_class": "filesystem",
  "authority_level": "write",
  "confused_deputy_candidate": true,
  "confidence": 0.91,
  "rationale": "name match \"\\b(write[_-]?file|edit[_-]?file|create[_-]?directory|move[_-]?file)\\b\" → filesystem/write (0.70); description match → filesystem/read (0.50); schema: arg \"path\" is path-shaped → filesystem (0.40); user-controllable string arg + non-read authority → confused-deputy candidate",
  "recommended_caveat": "tool == \"edit_file\" AND caller == \"<your-caller-id>\" AND arg.path starts_with \"<your-sandbox-prefix>/\" AND now <= @<your-cap-expiry>  // WRITE filesystem"
}
```

The full headline findings — including the `everything` server's DoS
surface and the `filesystem` wrapper's missing-bounds — are in
[`docs/WRITEUP.md`](docs/WRITEUP.md).

## Recon → capnagent in one pipe

```
   ┌──────────────┐    inventory.json     ┌──────────────┐
   │              │    fuzz.json          │              │
   │  MCP server  │ ──▶  classification ──▶│  capnagent   │ ──▶ deny anything
   │              │      .json            │   issuer     │     outside scope
   │              │      report.md        │              │
   └──────────────┘                       └──────────────┘
        ▲                                       │
        │                                       ▼
        └────────── scoped caller ◀──────  signed capability
```

mcp-recon documents the tool surface; capnagent enforces the bound.
Each project stands alone. Together they're a single security
posture for any MCP-shaped agent. Run mcp-recon first, paste the
suggested caveats into your capnagent issuer, ship.

### From recon to a capnagent issuer in one pipe

`classification.json` ships a copy-pasteable caveat per tool, but
manual paste is its own foot-gun. The `caveats` command produces a
machine-readable issuance plan ready to feed straight into a capnagent
issuer:

```bash
$ mcp-recon caveats ./reports/filesystem/classification.json \
    --caller=agent:planner \
    --sandbox-prefix=/var/agent-sandbox/tenant-42 \
    --expiry=2026-12-31T23:59:59Z \
    > ./reports/filesystem/caveats.json

mcp-recon: 14 plans (14 ready, 0 flagged) — schema=mcp-recon/v0.1/caveats
```

The output document (schema `mcp-recon/v0.1/caveats`) is one entry per
tool, with `caveats: string[]` already split into individual capnagent
DSL predicates and operator bindings substituted. Plans get flagged
with a structured reason set (`classification_unknown`, `low_confidence`,
`cdc_without_arg_constraint`, `unsubstituted_placeholder`) so the
review surface is machine-checkable.

Run with no bindings to get a "review pass" — every plan is flagged,
but you can see exactly which placeholders need binding before
committing values. Per-tool overrides (`per_tool_overrides` in the
library API) let you tighten confused-deputy candidates the
classifier didn't constrain.

## Why this exists

**For the developer adopting MCP.** Before you wire a third-party
MCP server into your agent, run mcp-recon against it. You get an
honest threat profile in 30 seconds — what does this thing
*actually* let an agent do, and what's the smallest cap that
preserves utility?

**For the security team auditing an agent stack.** mcp-recon turns
"we depend on N MCP servers" into "here's the consolidated tool
surface, here's what each one is classified as, here's where the
confused-deputy candidates are." A printable artifact you can
review.

**For the AI-security researcher.** mcp-recon's reports are the
input to round-N writeups in the
[capnagent purple-team corpus](https://github.com/euanmcrosson-dotcom/capnagent/tree/master/docs/purple-team).
Recon → capability gap → attack PoC → fix → CLOSED.

## Installation

```bash
# From source (the recommended path today; npm package is post-v0.2)
git clone https://github.com/euanmcrosson-dotcom/mcp-recon
cd mcp-recon
npm install
npm run -w @mcp-recon/cli build

# Run the CLI directly via tsx (no build step needed for development)
npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:npx -y @modelcontextprotocol/server-filesystem $HOME/sandbox" \
  --out=./reports/filesystem --budget=200
```

> **Windows / Git Bash users:** prefix path-shaped flags with `MSYS_NO_PATHCONV=1` to prevent leading-slash path mangling. Example: `MSYS_NO_PATHCONV=1 mcp-recon caveats classification.json --sandbox-prefix=/var/sandbox --expiry=2026-12-31T23:59:59Z`

## Documentation

- [`docs/SPEC.md`](docs/SPEC.md) — v0.1 surface, server-spec syntax, output schemas
- [`docs/METHODOLOGY.md`](docs/METHODOLOGY.md) — classifier rules, fuzz axes, signals, falsifiability
- [`docs/WRITEUP.md`](docs/WRITEUP.md) — public-dataset findings + headline observations
- [`schemas/`](schemas/) — formal JSON Schema files for the four wire formats
- [`findings/`](findings/) — corpus of documented findings (F001–F006)
- [`SECURITY.md`](SECURITY.md) — vulnerability reporting policy
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to add classifier rules, fuzz axes, dataset entries

## How it compares

|   | mcp-recon | [NVIDIA garak](https://github.com/NVIDIA/garak) | Burp / ZAP | manual review |
|---|---|---|---|---|
| **Scope** | MCP server tool surfaces | model-behavior testing | HTTP fuzzing | everything |
| **Output** | structured JSON + Markdown | reports | proxy logs | human prose |
| **Determinism** | yes (seeded PRNG) | partial | no | no |
| **LLM in the loop** | no (rules-based) | yes | no | yes |
| **OWASP LLM / MITRE ATLAS mapping** | yes (per-tool) | partial | no | author-dependent |
| **Companion enforcement** | [capnagent](https://github.com/euanmcrosson-dotcom/capnagent) | none | none | none |

mcp-recon is **not** a replacement for any of those — it's the
piece nobody else is building: a deterministic, schema-aware
characterization of an MCP server's tool surface, in a format
that wires straight into a capability-bounded enforcement layer.

## What this is NOT

- **Not a replacement for capnagent.** mcp-recon documents what's
  there; capnagent enforces what's allowed. You want both.
- **Not a vulnerability scanner for the model itself.** Use
  [NVIDIA garak](https://github.com/NVIDIA/garak) for that. We
  test the *tool surface*, not model behavior.
- **Not an exploitation framework.** We send adversarial schemas
  to characterize handling, not actual exploits.
- **Not a proxy / MITM tool.** Out of scope. See
  [`docs/SPEC.md`](docs/SPEC.md) §"What v0.1 does NOT do."

## Tests

The workspace has **68 unit + property-based tests** passing today
(`npm test`), covering schema parsing, the seeded PRNG, fuzz
generators along all six adversarial axes, the classification
rules, the Markdown report renderer, and end-to-end `scan` flow.
Two additional integration test files (`enumerate.integration.test.ts`,
`fuzz.integration.test.ts`) exercise live transport against a
locally-spawned MCP server when the dev environment provides one.

```bash
npm test           # all packages, vitest
npm run typecheck  # tsc --noEmit, strict mode
npm run lint       # biome check
```

## Companion project — capnagent

mcp-recon is the offensive complement to
[capnagent](https://github.com/euanmcrosson-dotcom/capnagent),
which provides capability-bounded authorization for AI agent tool
calls. Together they implement the standard
*recon-then-bound* security workflow:

```
[ mcp-recon ]  →  threat profile  →  [ capnagent ]
   "what is        "what should           "deny anything
    here?"          we allow?"             outside that"
```

Each project stands alone. Together they're a single security
posture for any MCP-shaped agent.

## License

[Apache-2.0](LICENSE).
