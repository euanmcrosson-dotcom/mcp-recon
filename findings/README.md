# mcp-recon findings corpus

This directory holds the load-bearing security observations produced
by running `mcp-recon scan` against the dataset in
[`examples/public-servers/`](../examples/public-servers/). It mirrors
the [capnagent purple-team-corpus](https://github.com/euanmcrosson-dotcom/capnagent/tree/master/docs/purple-team)
format: one Markdown file per finding, prefixed `F0NN-...`.

**A note on severity language.** Most findings here are *capability-
surface observations* — facts about what a published reference MCP
server lets an agent do, surfaced by the recon tool. They are not
"vulnerabilities in the upstream" unless the writeup explicitly
labels them so. The default framing is "this server exposes X; an
operator wiring it into an agent should bound Y via capnagent."

The bar for escalating to "vulnerability" is whether the upstream
maintainer would clearly accept it as one (a server crash, info
disclosure across the trust boundary, or a privilege escalation
that doesn't require the operator to opt in). Three of the six
findings below sit at that bar; three are observational.

## Index

| ID | Title | Server | Severity | Status |
|---|---|---|---|---|
| [F001](F001-server-everything-doslogging-amplification.md) | `trigger-long-running-operation` lacks upper-bound check | `@modelcontextprotocol/server-everything` | low | not-applicable-published-as-test-server |
| [F002](F002-server-puppeteer-selector-timeout-amplification.md) | Selector-taking tools amplify 5s timeout per call into trivial DoS | `@modelcontextprotocol/server-puppeteer` | medium | not-applicable-published-as-archived-example |
| [F003](F003-server-puppeteer-evaluate-privileged.md) | `puppeteer_evaluate` is arbitrary JS execution; recon flags `shell/privileged` | `@modelcontextprotocol/server-puppeteer` | informational | not-applicable-documented-as-evaluation-tool |
| [F004](F004-server-postgres-readonly-claim-undocumented-in-schema.md) | `query` tool description claims "read-only" but schema doesn't enforce | `@modelcontextprotocol/server-postgres` | low | not-applicable-published-as-deprecated-example |
| [F005](F005-server-git-repository-allowlist-positive-control.md) | `--repository` CLI flag enforces allowlist (positive control) | `mcp-server-git` (PyPI) | informational | n/a (positive observation) |
| [F006](F006-classifier-timezone-false-positive.md) | mcp-recon classifier mis-tags IANA timezone strings as filesystem paths | mcp-recon (this tool) | low | open |

## Summary by severity

- **medium (1):** F002 — DoS amplification with concrete reproduction
  in `server-puppeteer/fuzz.json`.
- **low (3):** F001, F004, F006. F001 and F004 are
  capability-surface observations; F006 is a real bug in the recon
  classifier itself.
- **informational (2):** F003 (documented behaviour an operator
  needs to know about) and F005 (a positive defence-in-depth
  pattern other servers should copy).

## Source / methodology

Every finding has a literal `mcp-recon scan` reproduction command.
Re-running the command on the cited server version with the cited
budget and seed (always `--budget=200`, default seed `0xC0FFEE`)
produces byte-identical artefacts in `examples/public-servers/`.
That's the reproducibility contract this corpus relies on; if a
finding can't be re-derived from the dataset, it doesn't belong
here.

The classifier rules that produced these findings are documented
in [`docs/METHODOLOGY.md`](../docs/METHODOLOGY.md). When a finding
points at a classifier rule that fired (or didn't fire), it cites
the rule by name so the reader can audit it.

## Coordinated disclosure status

None of the findings here trigger a coordinated-disclosure window
because:

- The Anthropic-published `@modelcontextprotocol/server-*` packages
  are explicitly published "for evaluation" (per the upstream
  README) and the test-server `everything` is documented as a
  feature exerciser, not a production server.
- `server-puppeteer` and `server-postgres` are in
  [`Hisma/servers-archived`](https://github.com/Hisma/servers-archived)
  with explicit "archived" status; their behaviour is documented as
  example-quality. Anthropic moved these out of the active reference
  set.
- F006 is a bug in mcp-recon itself — fix lands in this repo.

If a future finding lands that *does* warrant coordinated
disclosure (e.g., a memory-safety bug in an active reference
server), the writeup gets `Coordinated disclosure status:
upstream-notified <date>` at the top and the file is held
unpublished until the window closes. None of the current six
findings hit that bar.
