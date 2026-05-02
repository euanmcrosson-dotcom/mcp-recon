# mcp-recon

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Status: pre-alpha](https://img.shields.io/badge/status-pre--alpha-orange.svg)](docs/SPEC.md)

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

> **Status:** v0.1.1 shipped 2026-04-30. Public dataset of every
> stdio TypeScript MCP server in Anthropic's `@modelcontextprotocol/*`
> namespace audited. See [`docs/WRITEUP.md`](docs/WRITEUP.md) for the
> headline findings (DoS surface on `everything`,
> missing-bounds finding on `filesystem` example wrapper, full
> server-maturity ranking).

## What you get

```bash
$ mcp-recon scan "stdio:npx -y @modelcontextprotocol/server-filesystem /tmp" \
    --out=./reports/filesystem --budget=200

mcp-recon: 14 tools, 4 confused-deputy candidates
mcp-recon: fuzz — ok=4 protocol_error=719 runtime_error=0
mcp-recon: wrote 4 artefacts to ./reports/filesystem/

$ ls ./reports/filesystem/
inventory.json   fuzz.json   classification.json   report.md
```

Run against any of the 4 servers in the public dataset and your
output matches `examples/public-servers/server-<name>/` byte-for-
byte. See [`docs/EVALUATION.md` (in capnagent)](https://github.com/euanmcrosson-dotcom/capnagent/blob/master/docs/EVALUATION.md)
for the reproducibility contract.

The `scratch/report.md` is the deliverable a security reviewer or
developer-on-call actually reads. The JSON files are the machine-
parseable evidence the writeup links to.

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
MCP server into your agent, run mcp-recon against it. You get a
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
