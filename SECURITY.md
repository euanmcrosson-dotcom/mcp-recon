# Security Policy

mcp-recon is a security tool — it classifies third-party MCP servers, fuzzes
them with adversarial inputs, and emits capability caveats that downstream
issuers (e.g. capnagent) will accept. Bugs here can mislead a reviewer into
trusting an unsafe server, or produce caveats the operator did not intend.
We treat reports seriously and respond quickly.

## Reporting a vulnerability

**Preferred:** [Open a private security advisory](https://github.com/euanmcrosson-dotcom/mcp-recon/security/advisories/new)
on GitHub. This keeps disclosure private until a fix lands.

**Fallback:** email `euanmcrosson@gmail.com` with subject line beginning
`[mcp-recon-security]`. PGP key on request.

We aim to:

- **Acknowledge** within 72 hours.
- **Provide a remediation plan** within 14 days.
- **Coordinate disclosure** with you on a default 90-day timeline. Shorter
  is fine if a fix is straightforward; longer if the underlying issue is
  systemic. We will agree the timeline with you before any public posting.

If we cannot meet 72 hours, we will at least acknowledge receipt with a
revised timeline.

## What is in scope

- **Classifier bugs.** Either direction:
  - **False negative:** a confused-deputy candidate that the classifier
    failed to flag, where a reviewer can demonstrate the missed pattern
    on a real or constructed MCP server.
  - **False positive:** a benign tool misclassified as `privileged`,
    `confused-deputy`, or any other elevated label, where the
    misclassification is reproducible and not a tuning question.
- **Fuzzer bugs in adversarial-input generation.** For example:
  - Inputs that crash a target server in a way that leaks memory contents
    back through the recon output.
  - The fuzzer producing inputs that violate JSON-RPC framing in
    unexpected ways (e.g. unbalanced brackets that desync the stream
    parser, framing tricks that escape the intended message boundary).
- **`caveats` command bugs.** Anything that produces a caveat the
  operator's downstream capnagent issuer would accept that the operator
  did not intend. The AND-split bypass (known and fixed in PR #1) is the
  canonical example; future similar bugs are in scope.
- **CLI argument-parsing bugs.** Flag injection, argument smuggling
  through positional vs. flag confusion, anything that lets a quoted
  string in one argument inject a flag the user did not pass.
- **Transport-layer bugs.** Anything that allows a malicious MCP server
  to compromise the recon process via stdio framing tricks — buffer
  manipulation, chunk-boundary attacks, NDJSON ambiguity, or escape
  sequences that the recon process interprets rather than transmits.
- **Memory-safety issues** in any unsafe code that lands in the Rust
  crates. The workspace currently sets `unsafe_code = "forbid"`; any
  unsafe block introduced in future work is in scope.
- **Dependency-supply-chain compromises** observable from the locked
  `Cargo.lock` or `package-lock.json`.

## What is out of scope (for v0.1 / v0.2)

- Denial-of-service via large or malformed inputs. The fuzzer's own
  input is the bound here; resource caveats are a v0.6 backlog item in
  the companion capnagent project.
- Findings against documentation examples or test fixtures, unless they
  reveal a library-side bug.
- Side-channel weight extraction from any underlying ML models. mcp-recon
  does not touch model internals.
- Anything reproducible only against pre-release versions
  (`0.0.x`) that have already been superseded.

## Supported versions

| Version | Status |
|---|---|
| 0.0.x | Pre-release. Security fixes on a best-effort basis; no SLA on patch releases. |
| 0.1.x | Tier-2 support. Security-fixes-only after v0.2.0 ships. |
| 0.2.x | Tier-1 support. Current line. Patches within the disclosure window. |
| ≥ 0.3.0 | (planned) Tier-1 support continues on the latest minor line. |

## Reporting MCP server vulnerabilities found via mcp-recon

This is the case we expect most often: a reviewer runs mcp-recon against
a third-party MCP server and discovers what looks like a real
vulnerability in *that server*, not in mcp-recon itself.

**Where to report it.** Coordinate disclosure directly with the upstream
maintainer of the affected MCP server, following standard 90-day
timelines. Most maintainers accept reports via a `SECURITY.md` in their
repo or via a GitHub private security advisory; if neither exists, a
private email to the listed maintainer with a reasonable disclosure
deadline is the community norm.

**What this project will and will not accept.**

- The mcp-recon project does **not** accept reports about third-party
  servers. That conversation is between you (the reviewer) and the
  server's maintainer. We have no standing to triage, fix, or coordinate
  disclosure for code we do not maintain, and inserting ourselves would
  slow the upstream fix.
- The mcp-recon project **will** accept "the recon tool failed to flag
  this server pattern" reports. If mcp-recon's classifier missed a
  pattern that a reviewer can demonstrate is a real confused-deputy or
  privileged-tool risk, that is a classifier bug — see "What is in
  scope" §1 above. File it as a private advisory against mcp-recon, and
  separately disclose the underlying server bug to the server's
  maintainer.

If the upstream maintainer is unresponsive past the 90-day window,
publish the advisory yourself; the mcp-recon project may reference your
public writeup in a future classifier-rules update but will not
re-disclose on your behalf.

## Security model

The three load-bearing legs of mcp-recon's security argument are:

1. **Classifier honesty.** The `tier` label on a tool reflects the
   reviewer-relevant risk, not a heuristic that produces false comfort.
   Any report that demonstrates a tier label misleads a reviewer is in
   scope.
2. **Caveat fidelity.** A caveat emitted by the `caveats` command, when
   evaluated by the operator's downstream issuer, must reject every
   request the operator intended to reject. Bugs that produce
   over-permissive caveats are in scope (see the AND-split bypass in
   PR #1 as the prototype).
3. **Recon-process isolation.** A malicious MCP server cannot escape
   the recon harness via stdio framing, child-process behavior, or
   error-handling paths. The recon process must remain in control of
   what it runs and what it reports.

Any report that breaks one of those three legs is automatically in
scope, even if it does not fit cleanly into the bullet lists above.
