# F003 — `puppeteer_evaluate` is arbitrary JS execution; recon flags `shell/privileged`

| Field | Value |
|---|---|
| **Server** | `@modelcontextprotocol/server-puppeteer` v0.1.0 (npm `2025.5.12`) |
| **Tool** | `puppeteer_evaluate` |
| **Discovery date** | 2026-05-01 |
| **Severity** | informational |
| **Severity rationale** | Not a vulnerability — the tool's documented behaviour is "Execute JavaScript in the browser console", which is by design the most powerful surface a browser-automation MCP server exposes. Filed as informational because operators wiring this tool into an agent need to know that **it bypasses every URL allowlist they configure on `puppeteer_navigate`** (a `puppeteer_evaluate` of `window.location = ...` re-navigates without going through the navigate caveat). The recon classifier surfaces this with a `tool != "puppeteer_evaluate"` deny-by-default caveat — that's the behaviour we want to keep documenting. |
| **Coordinated disclosure status** | not-applicable-documented-as-evaluation-tool |

## Description

The mcp-recon classifier evaluates `puppeteer_evaluate` against
its rule set:

```
description match "\b(execute|shell|subprocess|invoke|launch[_-]?process)\b"
  → shell/privileged (0.50)
fuzz: 3/30 accepted → +0.1
user-controllable string arg + non-read authority → confused-deputy candidate
```

The classifier produces:

```json
{
  "tool": "puppeteer_evaluate",
  "data_class": "shell",
  "authority_level": "privileged",
  "confused_deputy_candidate": true,
  "confidence": 0.55,
  "recommended_caveat": "tool != \"puppeteer_evaluate\"  // PRIVILEGED — recommend deny outright; operator should hand-write argv allowlist if exposing"
}
```

This is exactly the behaviour we want for an arbitrary-code-execution
tool: the recommended caveat is *deny by default*, not an
`arg.script` allowlist. There is no schema constraint that lets a
caveat express "only safe JavaScript" — the classifier knows that
and refuses to invent one.

The fuzzer's 3/30 acceptance rate (with seed `0xC0FFEE`, budget=200)
includes successful `JSON.stringify(window.location)` evaluations
during the seed corpus walk, confirming the tool *does* execute
provided JS and *does* return its result.

## Reproduction

```bash
mcp-recon scan "stdio:npx -y @modelcontextprotocol/server-puppeteer" \
  --out=examples/public-servers/server-puppeteer --budget=200

# Read the classifier's recommendation:
node -e 'console.log(require("./examples/public-servers/server-puppeteer/classification.json").classifications.find(c=>c.tool==="puppeteer_evaluate"))'
```

Expected: a classification record with `authority_level: "privileged"`
and `recommended_caveat` starting with `tool != "puppeteer_evaluate"`.

## Why this is "informational" not higher

This is the rare case where the **classifier output is the
finding**: the tool is documented as arbitrary code execution and
the upstream is not promising bounded behaviour. The mcp-recon
output exists to make this fact explicit in a place an operator
will actually read (the threat profile next to the more-bounded
tools), so they don't accidentally treat `puppeteer_evaluate` as
"just another action" while writing capnagent caveats for
`puppeteer_click` / `puppeteer_fill`.

The bypass-of-navigate-allowlist note is the operationally-
important part: an operator who has carefully written

```
tool == "puppeteer_navigate" AND arg.url == "https://corp.example/login"
```

…and forgotten to deny `puppeteer_evaluate` has built a
defence-in-depth illusion. Both must be caveated; the classifier's
default `tool != "puppeteer_evaluate"` is the safe fallback.

## Recommended capnagent caveat

```
tool != "puppeteer_evaluate"
  // PRIVILEGED — recommend deny outright. If exposure is required,
  // hand-write a fixed-script allowlist (`arg.script in ["return document.title", ...]`)
  // and document the threat-model exception in the agent README.
```

The classifier's verbatim output is fine to use as-is. The
operator-side discipline is to leave the deny in place rather
than relax it to `arg.script != ""` or similar non-bound.
