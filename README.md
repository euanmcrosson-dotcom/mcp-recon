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

> **Status:** pre-alpha scaffold. v0.1 ships in 4 weeks against
> the [SPEC](docs/SPEC.md). Don't depend on the surface yet.

## What you get

```bash
$ mcp-recon scan stdio:npx @modelcontextprotocol/server-filesystem /tmp

# 6 tools enumerated:
#   read_file        — filesystem (read)        — confused-deputy candidate
#   list_directory   — filesystem (read)        — safe
#   directory_tree   — filesystem (read)        — safe
#   write_file       — filesystem (write)       — high-risk; bound aggressively
#   create_directory — filesystem (write)       — moderate
#   delete_path      — filesystem (write)       — high-risk
#
# Fuzzer ran 1200 calls (200/tool). 14 schema-violations passed
# through to the underlying tool — see scratch/fuzz-results.json
#
# Threat profile: scratch/report.md
# Suggested capnagent caveats: scratch/caveats.txt
```

The `scratch/report.md` is the deliverable a security reviewer or
developer-on-call actually reads. The JSON files are the machine-
parseable evidence the writeup links to.

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

## Installation (when v0.1 ships)

```bash
# From npm (pending v0.1 release)
npm install -g @mcp-recon/cli

# From source (today)
git clone https://github.com/euanmcrosson-dotcom/mcp-recon
cd mcp-recon
npm install
npm run build
node packages/mcp-recon-cli/dist/bin/recon.js scan stdio:...
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
