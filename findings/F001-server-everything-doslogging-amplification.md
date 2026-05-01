# F001 — `trigger-long-running-operation` lacks an upper-bound check

| Field | Value |
|---|---|
| **Server** | `@modelcontextprotocol/server-everything` v2.0.0 (npm `2026.1.26`) |
| **Tool** | `trigger-long-running-operation` |
| **Discovery date** | 2026-04-29 (re-confirmed 2026-04-30 at budget=200) |
| **Severity** | low |
| **Severity rationale** | Capability-surface observation against an explicitly-published *test* server. The upstream README labels `everything` as a feature-exerciser, not a production target, and the tool's literal name is `trigger-long-running-operation` — the behaviour is by design. The finding is "low" because it documents a DoS amplification surface that an operator who copies tool wrappers from `everything` into a real server might inherit by mistake. |
| **Coordinated disclosure status** | not-applicable-published-as-test-server |

## Description

The `trigger-long-running-operation` tool advertises a `steps` and a
`duration` parameter for simulating long-running work. Its
`inputSchema` declares both as `integer` but does not constrain the
upper bound. The mcp-recon fuzzer's boundary axis feeds
`Number.MAX_SAFE_INTEGER` (`9007199254740992`) and similar large
values; the server faithfully attempts to execute the requested
iteration count rather than rejecting it, and the MCP transport
times out after 5 seconds with `timeout after 5000ms`. The
classifier flags the surface as `compute/write` with confidence
0.50; the runtime_error count is what makes it interesting.

At `--budget=200` the seed-`0xC0FFEE` fuzzer hits **7 separate
timeout events** on this single tool — every other tool in the
9-server dataset combined produces 0. The pattern is reproducible
(`grep -c '"runtime_error"' examples/public-servers/server-everything/fuzz.json`
returns the same count on every re-run).

## Reproduction

```bash
mcp-recon scan "stdio:npx -y @modelcontextprotocol/server-everything" \
  --out=examples/public-servers/server-everything --budget=200
```

Expected output (from `examples/public-servers/server-everything/fuzz.json`):

```json
{
  "tool": "trigger-long-running-operation",
  "arguments": { "steps": 9007199254740992 },
  "outcome": { "kind": "runtime_error", "message": "timeout after 5000ms" }
}
```

The `report.md` summary shows `Fuzz: 371 calls — ok=86,
protocol_error=278, runtime_error=7`. The 7 runtime_errors are all
on this one tool.

## Recommended capnagent caveat

The classifier emits this caveat directly into
`server-everything/classification.json`:

```
tool == "trigger-long-running-operation"
  AND caller == "<your-caller-id>"
  AND arg.steps <= 100
  AND arg.duration <= 5
  AND now <= @<your-cap-expiry>
```

The bounded `arg.steps` and `arg.duration` predicates are the
finding-specific tightening — without them, an injected tool call
can park a worker thread in 5-second timeouts.

## Why this is "low" not "medium"

A medium-severity DoS would meet two extra criteria: (a) the server
is published for production use, and (b) the bound is missing
*despite* documentation that promises validation. Neither holds for
`everything` — it's labelled a test server and the tool's literal
name advertises long-running behaviour. The value of the finding
is as a *concrete reproduction* the classifier can cite when an
operator copies this shape into a server they actually deploy.
