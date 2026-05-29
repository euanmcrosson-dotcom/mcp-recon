# Sandbox producer — design

The static-manifest producer (`mcp-recon producer registry`) reads
package metadata + READMEs without executing code. That's cheap and
broad — every published MCP package can be graded — but it's lossy:
parameter schemas don't survive, tool-level descriptions are
best-effort, and packages that don't document tools in a recognisable
README pattern get fallback synthesis only.

The sandbox producer trades cost for fidelity. Each corpus entry runs
in an ephemeral microVM, installs the package, spawns the MCP server
over stdio, performs the canonical `initialize` + `tools/list`
handshake, and captures the *live* tool surface. R1/R2/R4 fire on real
parameter schemas. R3/R5/R6/R7 fire on the server's self-declared
names + descriptions, not heuristic README parsing.

## Why the sandbox is non-negotiable

Static analysis can't enumerate stdio MCP servers. They expose tools
through JSON-RPC at runtime, not through any installable manifest. The
only way to get a high-fidelity surface from a package you don't
trust is to run it in a sandbox you control.

## Status

**Scaffold.** `crates/mcp-recon-cli/src/producer/sandbox.rs` declares
the public surface but `fetch_server` returns an explicit
`not yet wired` error. The provider integration is the next phase;
this doc + the scaffold lock the contract so the rest of the pipeline
(corpus walk, envelope, aggregator, leaderboard) needs zero changes
when the implementation lands.

## Pipeline shape

```
  corpus entry
       │
       ▼
  parse handle (npm:<name>@<version> or pypi:<name>@<version>)
       │
       ▼
  provision microVM   ──── provider API ──── Vercel Sandbox / Firecracker
       │                                      Docker (local fallback)
       ▼
  install package (npm install <name>@<version> OR pip install <name>==<version>)
       │
       ▼
  spawn stdio server with the bin / entry_point
       │
       ▼
  JSON-RPC handshake:
    -> initialize
    <- (capabilities)
    -> notifications/initialized
    -> tools/list
    <- { tools: [{name, description, inputSchema}, ...] }
       │
       ▼
  shutdown subprocess + tear down VM
       │
       ▼
  emit McpServer { tools: [Tool {name, description, parameters, ...}] }
       │
       ▼
  existing classify() runs unchanged
       │
       ▼
  findings.v2 with server.source = "sandbox"
```

## Provider choice

The default in `SandboxConfig` is `vercel` because it's an
already-shipped Firecracker-on-API offering with a pay-per-use model
matching this workload (one-shot, ~30s-2min per package). Alternatives:

- **Vercel Sandbox** — first-class Firecracker microVMs, simple API.
- **AWS Fargate / GCP Cloud Run / Fly Machines** — heavier provision
  times; better suited to long-lived isolation than one-shot ephemeral.
- **Local Docker** — fine for development and small corpora; doesn't
  scale to a daily 1000-server walk and gives operators no isolation
  from a malicious package.

## Cost envelope

Rough order of magnitude (Vercel Sandbox pricing as of 2026-05):

- ~$0.0001 / vCPU-sec
- One scan ≈ 30-90 seconds wall-clock
- 1000 corpus entries → 1000 × 60s × $0.0001 ≈ $6 per full walk
- Weekly cadence → ~$25/month at corpus size 1000

The registry producer covers the daily cron; the sandbox producer runs
weekly (configurable) and *augments* the registry findings rather than
replacing them. Leaderboard rows show both sources where both exist;
ties are broken in favour of the higher-fidelity sandbox source when
reconciling.

## What's not covered by the sandbox path

- Packages that require external credentials at install or runtime
  (e.g. MCP server needing `STRIPE_API_KEY` to initialize). These fail
  at the handshake step and degrade back to the registry producer's
  output. Acceptable for a leaderboard.
- Packages with malicious install scripts. The sandbox is the
  containment boundary — that's the entire point.
- Per-tool runtime invocation. The producer captures the *declared*
  tool surface (`tools/list`), not actual call behaviour. Capframe
  Guard is the runtime layer; this is the Find / grade layer.

## When this ships

The static-manifest path is good enough for v1 leaderboard signal.
Real sandbox wiring lands when:

1. The static path is exhausted of low-hanging extensibility (deeper
   README parsing, sidecar schema files, etc.)
2. The corpus exceeds ~500 entries and the leaderboard needs more
   score variance to be useful as a public ranking
3. The product positioning requires the live-tool surface (eg. for a
   paid CISO-facing report rather than a public OSS leaderboard)

Until then, the scaffold sits here as the canonical contract.
