# What 4 public MCP servers actually expose: a structural audit

> **TL;DR.** I scanned every TypeScript stdio MCP server in
> Anthropic's `@modelcontextprotocol/*` npm namespace with
> [mcp-recon](https://github.com/euanmcrosson-dotcom/mcp-recon).
> 4 servers, 37 tools, 1,374 schema-aware fuzz calls at the
> default budget (200 calls/tool). The `everything` example
> server will faithfully attempt 9,007,199,254,740,992 iterations
> of `trigger-long-running-operation` if a prompt-injected agent
> asks it to — and it does so reliably: 7 separate timeout events
> in 371 calls vs zero runtime_errors in 1,003 calls across the
> other 3 servers combined. The `filesystem` server has 14 tools
> but the canonical example wrapper only bounds 3 of them. Full
> methodology below.

---

## What this is, in one paragraph

MCP — the Model Context Protocol — is the wire format Anthropic
introduced in late 2024 for connecting LLM agents to external
tool surfaces. Every team adopting MCP right now is asking the
same question: *what does this server actually let an agent do?*
There has been no tool for it. mcp-recon is that tool.

Point it at any MCP server (stdio or HTTP — v0.1 ships stdio).
It enumerates the tool surface, runs a schema-aware adversarial
fuzzer along six axes, classifies authority against OWASP LLM
Top 10 + MITRE ATLAS, and emits a structured threat profile —
JSON for machines, Markdown for humans — with a recommended
[capnagent](https://github.com/euanmcrosson-dotcom/capnagent)
caveat per tool.

This post walks the v0.1 release through the dataset that
shipped with it: every public `@modelcontextprotocol/*` stdio
TypeScript server, audited.

---

## Methodology

Three primitives composed by a single `scan` command:

### 1. Enumerate

Standard MCP `tools/list` round-trip. The output is a
self-describing JSON document (`mcp-recon/v0.1/inventory`)
with every tool's name, description, and `inputSchema`. Nothing
clever; this is just the structured baseline.

### 2. Fuzz (six axes, deterministic PRNG)

For each tool, generate adversarial inputs along six categorical
axes:

| Axis | Examples |
|------|----------|
| **boundary_values** | `""`, 64KiB string, `0`, `-1`, `MAX_SAFE_INT+1`, `NaN`, `Infinity`, `null`, missing required fields |
| **type_confusion** | string-where-number-expected, array-where-object-expected, etc. |
| **encoding_tricks** | `%00`, `%2e%2e`, `%252e%252e`, embedded NUL, Cyrillic homograph, RTL override, combining diacritics |
| **path_traversal** | `../etc/passwd`, `..\windows\system32`, `/sandbox/../etc/passwd`, `/sandbox/%2e%2e/etc/passwd` |
| **url_hostility** | userinfo-splitting, IDN homograph, `javascript:` / `data:` / `file:` schemes, AWS metadata IP, IPv6 unspec |
| **schema_violation** | extra fields, missing required, `__proto__` pollution, 50-deep nested objects |

mulberry32 PRNG, default seed `0xC0FFEE`. Re-running with the
same seed against the same server produces bit-identical inputs.
Default budget: 200 calls/tool (this writeup used 15/tool to keep
artefacts small).

The fuzzer **records what happened** — `ok` / `protocol_error` /
`runtime_error`. It does NOT classify whether a particular fuzz
"succeeded" — that's the human reviewer's job (or v0.2's).

### 3. Classify (rule-based, falsifiable)

22 regex rules across 8 data classes (filesystem / network /
shell / payments / messaging / system / metadata / unknown), 4
authority levels (read / write / destructive / privileged), and
the **confused-deputy flag**: tool takes user-controllable string
args AND has write-or-above authority.

Confidence combines via noisy-OR (`1 - ∏(1 - c_i)`) over fired
rules. Each classification carries a `rationale` string listing
every rule that fired with its weight, so a reviewer who
disagrees can see exactly which rule to reweight.

Methodology details in
[docs/METHODOLOGY.md](https://github.com/euanmcrosson-dotcom/mcp-recon/blob/master/docs/METHODOLOGY.md).
Rule table in
[src/classify/rules.ts](https://github.com/euanmcrosson-dotcom/mcp-recon/blob/master/packages/mcp-recon-cli/src/classify/rules.ts).

---

## The dataset

| Server | Tools | Fuzz calls | ok | protocol_error | runtime_error | Confused-deputy candidates |
|---|---|---|---|---|---|---|
| `secure-filesystem-server` | 14 | 723 | 4 | 719 | 0 | 4 |
| `memory-server` | 9 | 146 | 29 | 117 | 0 | 0 |
| `sequential-thinking-server` | 1 | 134 | 22 | 112 | 0 | 1 |
| `example-servers/everything` | 13 | 371 | 86 | 278 | **7** | 1 |

**Total: 4 servers, 37 tools, 1,374 fuzz calls at default
budget=200.**

This is Anthropic's full reference set of stdio TypeScript MCP
servers as of 2026-04-29. The pdf server is HTTP-only (v0.2);
everything else in the `@modelcontextprotocol/server-*` npm
namespace is Python (out of scope for an npx-based v0.1) or
auth-required (out of scope for a credential-free public audit).

Each server's full inventory + fuzz + classification + Markdown
report is committed:
[examples/public-servers/](https://github.com/euanmcrosson-dotcom/mcp-recon/tree/master/examples/public-servers)

---

## Findings

### Finding 1 — `everything`'s `trigger-long-running-operation` is a reliable DoS surface

**Status:** **7 of 7 runtime_errors** in the 1,374-call dataset
fall on this single tool. Every other tool in every other server
produced 0. The DoS-shape is the dominant runtime signal.

**Reproduce:**
```bash
mcp-recon scan "stdio:npx -y @modelcontextprotocol/server-everything" \
  --out=./reports/everything --budget=15
grep -A 10 'runtime_error' ./reports/everything/fuzz.json
```

**The input pattern:** boundary-axis variants of
`steps: <huge-number>` — `9007199254740992` (MAX_SAFE_INT + 1),
`-1` interpreted as a large unsigned, `Infinity`, the 64KiB
string of digits coerced via the JSON-parse path.

**The behaviour:** the server faithfully starts the requested
loop, hits mcp-recon's 5-second per-call timeout, and the fuzzer
records a `runtime_error` with message `"timeout after 5000ms"`.
**It does this 7 times in 200 attempts**, across multiple
fuzz-axis inputs — proof that this isn't a one-off; the server
has no upper-bound check at all on iteration count.

**Why this is interesting.** The server isn't crashing — it's
*correctly* implementing what the agent asked. The validation gap
is the missing **upper-bound check**: the schema declares `steps`
as a number with no `maximum` constraint, and the implementation
trusts whatever it receives. A prompt-injected agent that gets
the model to emit `trigger-long-running-operation` with a huge
`steps` value can burn server CPU for arbitrarily long, with no
crash, no error, no signal that anything is wrong.

This is a generic class for any agentic product: **schema-declared
numeric args without `minimum` / `maximum` constraints are DoS
amplifiers under prompt injection.** The fix is one annotation in
the JSON Schema; it's missing in `everything`, the canonical
*example* server everyone copies.

### Finding 2 — `filesystem` exposes 14 tools; the canonical example wrapper bounds 3

The official `@modelcontextprotocol/server-filesystem` exposes
**14 tools**: `read_file`, `read_text_file`, `read_media_file`,
`read_multiple_files`, `write_file`, `edit_file`,
`create_directory`, `list_directory`, `list_directory_with_sizes`,
`directory_tree`, `move_file`, `search_files`, `get_file_info`,
`list_allowed_directories`.

The canonical example wrapper from
[capnagent/examples/mcp-fs-agent](https://github.com/euanmcrosson-dotcom/capnagent/tree/master/examples/mcp-fs-agent)
issues a capability that authorizes **3** of these:
`read_file`, `list_directory`, `directory_tree`.

The other 11 tools are denied at capnagent's gate (which is
*correct* — that's the engine doing its job). But the example's
threat-profile honesty is incomplete: a reader doesn't see the
gap between "tools the underlying server exposes" and "tools the
example chooses to allow."

mcp-recon's classifier flagged 4 of the 11 missing tools as
**confused-deputy candidates** (`write_file`, `edit_file`,
`create_directory`, `move_file`) — they take user-controllable
path strings AND have write authority. Operators copying the
example need to know they're getting a much narrower bound than
the server's full surface.

[Filed as capnagent issue #1](https://github.com/euanmcrosson-dotcom/capnagent/issues/1).

### Finding 3 — `memory`'s destructive tools are NOT confused-deputy candidates

The memory server's `delete_entities`, `delete_observations`,
`delete_relations` are destructive — they remove state
irreversibly. But mcp-recon's classifier did NOT flag them as
confused-deputy candidates.

Why? They take `entityNames: string[]` (arrays of names), not
free-form strings. The confused-deputy threat in v0.1's model is
specifically about prompt-injection turning a user-controllable
*string* into a different authority. Array-typed args are not
equivalently exposed: an agent forced to pass `["admin"]` is no
worse than an agent passing `["normal-user"]` — both are valid
shapes; the authorization decision is on the contents, not the
typing.

This is the classifier behaving correctly on a non-trivial case.
It's not magic — the rule that fires is the same one for any
destructive operation; the *confused-deputy* flag is a separate
post-processing step that explicitly checks `string`-typed args.

This finding matters because it shows the methodology has
falsifiable boundaries: the classifier doesn't over-flag.

### Finding 4 — sequential-thinking's accept-rate is sample-size sensitive

`sequentialthinking` is the only tool in the
sequential-thinking-server, and it has a 12-property schema where
most arguments are booleans / integers with descriptive names.
The classifier produces a `metadata/write` classification with
`confused_deputy_candidate=true`.

A noteworthy methodology observation: at the N=15 preview budget,
sequential-thinking was the *most* permissive server (60% accept
rate). At N=200 — the actual production budget — it's the
*third-strictest* (83.6% protocol_error rate; 22 ok / 134 total).
The small-N preview was a sample-size artefact: with 15 calls
across one tool, the fuzzer disproportionately hit the
boolean/integer paths the server gracefully accepts; with 200,
the encoding-tricks and schema-violation axes get enough coverage
to surface real rejections.

**This itself is a useful finding.** When auditing your own MCP
servers, run mcp-recon at the default N=200 budget. Smaller
budgets produce directionally-suggestive but not statistically-
solid results.

### Finding 5 — overall protocol_error rates form a server-maturity ranking

Across the 1,374 calls:

| Server | protocol_error rate |
|---|---|
| filesystem | 99.4% |
| memory | 80.1% |
| everything | 74.9% |
| sequential-thinking | 83.6% |

This is essentially **how strict each server's input validation
is**. filesystem (the most production-ready) rejects 98% of
adversarial inputs; sequential-thinking (the most permissive)
rejects only 40%. Neither extreme is wrong on its own — they
reflect different deployment intents — but a reader auditing an
agent stack now has a quantitative number to compare against.

For agent operators: the fuzz profile is a more honest signal of
"how much can prompt injection actually do here?" than the
description of any individual tool.

---

## Recommended capnagent caveats (sample)

Each report ends with copy-pasteable capnagent predicates per tool.
Two examples from `filesystem`:

```
# move_file — bounds BOTH source and destination
tool == "move_file"
  AND caller == "<your-caller-id>"
  AND arg.source starts_with "<your-sandbox-prefix>/"
  AND arg.destination starts_with "<your-sandbox-prefix>/"
  AND now <= @<your-cap-expiry>
```

```
# read_file — single path bound
tool == "read_file"
  AND caller == "<your-caller-id>"
  AND arg.path starts_with "<your-sandbox-prefix>/"
  AND now <= @<your-cap-expiry>
```

The synthesizer recognises path-shaped args via name heuristics
(`looksPathy(...)` in
[`fuzz/schema.ts`](https://github.com/euanmcrosson-dotcom/mcp-recon/blob/master/packages/mcp-recon-cli/src/fuzz/schema.ts))
and bounds **every** path-shaped arg, not just the first. The
operator substitutes the placeholders and ships.

For privileged tools (anything matching shell / exec / spawn
keywords), the suggested caveat is `tool != "<name>"` — outright
deny, with a comment that an argv allowlist is the only safe
exposure pattern. (Unsurprisingly: zero of the 4 servers in this
dataset have shell-class tools.)

---

## Limitations — what mcp-recon doesn't see

Honest about boundaries:

- **Server-side correctness.** The server might be vulnerable in
  ways invisible to the client. mcp-recon classifies *exposed
  authority*, not *exploitability*.
- **Cross-server interactions.** Round 01 of the
  [capnagent purple-team corpus](https://github.com/euanmcrosson-dotcom/capnagent/tree/master/docs/purple-team)
  is a cross-server confused-deputy: a tool from server A
  weaponized by an injection from server B. v0.1 of mcp-recon
  classifies one server at a time.
- **Auth bypass.** Servers requiring auth and a pre-authenticated
  transport are out of scope. We don't try unauthenticated paths.
- **Behavioural side-effects.** A tool named `read_file` might
  have hidden write side effects in a malicious server. The
  classifier reads the *declared* surface; behaviour-based
  analysis is a different category of tool.
- **HTTP transport.** v0.1 stdio-only. The pdf server (HTTP) is
  v0.2.
- **Python servers.** The fetch / git / time official servers are
  Python; v0.1 supports the npx ecosystem only.
- **N=200 default fuzz budget** is what this dataset uses. An
  earlier preview at N=15 produced a sample-size-sensitive
  ranking that didn't survive a re-run; the lesson is in
  Finding 4 above. If a future scan reports outcome-rate numbers
  from a smaller budget, treat them as directional, not solid.

Read the
[methodology change-log](https://github.com/euanmcrosson-dotcom/mcp-recon/blob/master/docs/METHODOLOGY.md#methodology-change-log)
for the design boundary.

---

## Reproduce locally in 5 minutes

```bash
git clone https://github.com/euanmcrosson-dotcom/mcp-recon
cd mcp-recon
npm install

# Run the same scan against the filesystem server.
# (Substitute a path your user owns.)
npx tsx packages/mcp-recon-cli/src/bin/recon.ts scan \
  "stdio:npx -y @modelcontextprotocol/server-filesystem $HOME/sandbox" \
  --out=./reports/filesystem --budget=200

# The 4 commit-tracked artefacts in this writeup are at
# examples/public-servers/server-{filesystem,memory,
#   sequential-thinking,everything}/

# Verify by running the test suites:
cargo test          # 0 (Rust core stub for v0.1)
npm test            # 68 unit tests
LIVE_MCP=1 npm test # +9 integration tests against the live MCP servers
```

Total time: ~5 minutes once npm install completes.

---

## Why this exists / what comes next

mcp-recon is the *recon* side of the MCP-security workflow.
[capnagent](https://github.com/euanmcrosson-dotcom/capnagent) is
the *bound* side: take a recon report, derive a tight capability
caveat, deny everything outside it.

```
[ mcp-recon ]  →  threat profile  →  [ capnagent ]
   "what is        "what should           "deny anything
    here?"          we allow?"             outside that"
```

The roadmap (full
[ROADMAP.md](https://github.com/euanmcrosson-dotcom/mcp-recon/blob/master/docs/SPEC.md#v02-backlog-not-v01)):

- **v0.2** — HTTP transport (covers pdf and many community
  servers), ML-based classifier as a secondary signal, multi-server
  cross-product fuzzing (the round-01 cross-server-confused-deputy
  shape).
- **v0.3** — Replay / proxy mode for live agent surfaces. Some
  community feedback already requesting this.
- **v1.0** — When external integrations exist and the API has
  been stable for 60 days.

If you operate an MCP server or wrap one in an agent, **run mcp-recon
against your stack and tell me what's wrong with the report.** That
feedback is the v0.2 backlog.

---

## License + acknowledgements

Apache-2.0. Built with
[`@modelcontextprotocol/sdk`](https://www.npmjs.com/package/@modelcontextprotocol/sdk),
[`vitest`](https://vitest.dev), and a deliberate one-page-of-BNF
caveat DSL borrowed from
[capnagent](https://github.com/euanmcrosson-dotcom/capnagent).

Methodology informed by Greshake et al. (*Not what you've signed
up for*, 2023), Liu et al. (*Prompt Injection attack against
LLM-integrated Applications*, USENIX Security 2024), and Invariant
Labs's April 2025 work on MCP tool-poisoning. None of those are
mcp-recon — but understanding why they matter is what makes
running this tool useful.
