# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.4] — 2026-05-28

### Added

- **`mcp-recon mcp-server` — mcp-recon speaks the protocol it scans.** New
  stdio MCP server subcommand exposing the core classifier as two MCP tools
  any agent can call:
  - `classify_inventory(inventory)` → `findings.v1` JSON
  - `caveats(inventory)` → `mcp-recon/v0.1/caveats` JSON

  Newline-delimited JSON-RPC 2.0 per the MCP 2025-03-26 spec. Tool-name
  validated before inventory parsing so typos surface as `METHOD_NOT_FOUND`
  rather than confusing `INVALID_PARAMS` on the inventory; notifications
  return `None` uniformly via `req.id?`. Single file, no new dependencies,
  hand-rolled JSON-RPC.

  Drop into your `claude_desktop_config.json` as
  `{ "command": "mcp-recon", "args": ["mcp-server"] }` and the recon
  classifier becomes a tool any MCP-aware agent (Claude Desktop, Cursor,
  your own framework) can invoke. Also clears the listing-requirement bar
  for the [`awesome-mcp-servers#5754`](https://github.com/punkpeye/awesome-mcp-servers/pull/5754)
  Glama check, which previously couldn't introspect mcp-recon because it
  had no MCP server interface to talk to.

- **Adapters: classify non-MCP tool surfaces.** New
  `mcp-recon adapt --format anthropic|openai|langchain <input>` subcommand
  converts third-party tool-use payloads into `mcp-recon.inventory.v1` so
  the existing deterministic classifier runs unchanged on:
  - **Anthropic tool-use** — `[{ name, description, input_schema }, ...]`
  - **OpenAI function-calling** — both the current
    `[{ type: "function", function: {...} }, ...]` chat-completions shape
    AND the deprecated bare-function `[{ name, description, parameters }, ...]`
    form (you can even mix them in the same file)
  - **LangChain `BaseTool` dump** — `[{ name, description, args_schema }, ...]`
    with a fallback to bare `args`; for stacks that emit via
    `convert_to_openai_tool()`, the OpenAI adapter is the byte-for-byte
    match instead

  All formats accept either a bare array or a `{ "tools": [...] }` wrapper.
  Clean `AdapterError` enum surfaces useful messages for malformed input
  rather than opaque serde errors. New `mcp-recon-core::adapters` module
  exposes `from_anthropic_tools`, `from_openai_tools`, `from_langchain_tools`
  as library functions so downstream tools (e.g. capframe) can adapt
  in-process. Realistic example fixtures shipped at
  `examples/{anthropic,openai,langchain}-tools.json`.

  What's lost in translation — `side_effects`, `auth_required`,
  `rate_limited` aren't declared in any of these formats — is left empty
  and recovered by the classifier via R3 (name implies mutation), R5
  (description mentions money), R6 (description implies external fetch),
  and R7 (code execution).

### Tests

- Core lib: 45 → 63 (12 Anthropic+OpenAI adapter tests + 6 LangChain).
- CLI: subprocess integration tests for the MCP server and all three
  adapters against committed fixtures.
- Full workspace: 76 → 97 passing across the three landed PRs (#37, #38,
  #39); clippy clean throughout.

## [0.2.3] — 2026-05-28

### Fixed

- **Find→Bind handoff: tool names with control characters no longer produce
  unparseable caveats.** `caveats_v1` previously formatted tool names into
  `tool == "…"` / `tool != "…"` predicates with Rust's `Debug` formatter
  (`{:?}`). `Debug` emits a strict superset of the escape sequences
  capnagent's caveat DSL parser accepts — capnagent only knows `\n`, `\t`,
  `\\`, `\"`, while `Debug` can also emit `\r`, `\0`, and `\u{..}` for
  control characters. A tool name containing any such character produced a
  caveat string that failed parsing on the receiving end, silently breaking
  the handoff. Since the upstream MCP server controls tool names, a hostile
  or buggy server could turn this into a denial of the whole capability
  binding step.

  The serializer now uses an explicit `dsl_string_literal` that emits only
  the four escapes capnagent supports, and **fails closed** for any other
  control character: the affected tool gets a `recommend: "deny"` plan with
  an empty `caveats` list and an explanatory `note`, so the issuer cannot
  bind any capability covering the tool. Rule provenance is preserved on
  the fail-closed plan. Non-ASCII passes through unchanged — the DSL parser
  does not restrict it. Test corpus expanded with a vendored equivalent of
  capnagent's `parse_string` so every emitted predicate is round-trip
  asserted against the exact grammar the consumer enforces.

  Core lib tests: 35 → 45.

## [0.2.2] — 2026-05-15

### Changed

- **npm package renamed from `@mcp-recon/cli` to `mcp-recon`** (unscoped).
  v0.2.1's publish failed with `404 Scope not found` — the `@mcp-recon`
  npm scope doesn't exist (would require creating an npm org first).
  Rather than create the org just to scope a single CLI package, the
  package moves to the unscoped name `mcp-recon` (which we'd already
  verified was available).
- **End-user-facing payoff**: `npx mcp-recon ...` works directly
  instead of `npx @mcp-recon/cli ...`. Matches the project's
  long-stated naming and integrates cleanly with the three-layer
  stack table.
- Workspace internal references updated: root `package.json` `recon`
  script, `.github/workflows/bench.yml` + `publish-npm.yml` workspace
  selectors. Source code, docs, and case studies that mention
  "mcp-recon" by repo / project name were already correct.

## [0.2.1] — 2026-05-15

### Added

- **npm distribution.** The `@mcp-recon/cli` package now ships to npm
  on every GitHub Release. After this version is published, end users
  can run `npx @mcp-recon/cli ...` directly — no clone, no Rust
  toolchain, no TypeScript. Same three-layer-stack distribution
  parity as `capnagent` (PyPI) and `mcp-guardrails` (PyPI).
  - New `.github/workflows/publish-npm.yml` — release-triggered npm
    publish via `setup-node@v4` with `NODE_AUTH_TOKEN`. Includes
    `--provenance` for SLSA-style build attestation.
  - Gracefully no-op-skips when `NPM_TOKEN` secret isn't set,
    leaving the tarball as a workflow artifact for manual upload.

### Changed

- `packages/mcp-recon-cli/package.json` reshaped for npm consumption:
  - `main` / `types` / `bin` rewired from `./src/*.ts` to `./dist/*.js`
    so the published package is consumable without a TypeScript
    toolchain. The `prepublishOnly` script runs `npm run build` so
    `dist/` is always fresh on publish.
  - Added `files: ["dist", "README.md", "LICENSE"]` — drops the
    tarball from 116 kB / 145 files to 58 kB / 95 files by excluding
    `src/`, tests, benches, and tsconfig.
  - Added `keywords`, `homepage`, `repository.directory`, `bugs`,
    `engines`, `publishConfig.access` — standard npm metadata suite.
  - Added `exports` field for modern TS/ESM consumers.
- Workspace package version 0.2.0 → 0.2.1 (no behaviour change;
  first version actually publishable to npm).

## [0.2.0] — 2026-05-03

### Highlights

Closes the recon → capnagent loop. The new `mcp-recon caveats` command turns a
classification document into a capnagent-ready issuance plan in one pipe. The
public dataset doubles in size (4 → 9 servers) and ships with 6 documented
findings. The repo now has CI (Node 20+22 + Rust + dataset-validate), formal
JSON Schemas for all four wire formats, performance benchmarks, an adversarial-
server fixture harness, and a contributor + security policy. F006 (a real
classifier false-positive on IANA timezone strings) was found and fixed in
this release.

### Added — Caveats command + library

- **`mcp-recon caveats <classification.json> [--caller] [--sandbox-prefix] [--expiry] [--markdown]`** —
  sixth subcommand. Consumes `mcp-recon/v0.1/classification` and emits a
  `mcp-recon/v0.1/caveats` document of capnagent-ready issuance plans with
  placeholder substitution and structured flags (`classification_unknown`,
  `low_confidence`, `cdc_without_arg_constraint`, `unsubstituted_placeholder`).
  Closes the manual-paste loop between recon and capnagent. (#1)
- **`planCaveats(classification, bindings)`** library API plus types
  (`CaveatBindings`, `CaveatPlan`, `CaveatsResults`, `FlagReason`,
  `CAVEATS_SCHEMA`). (#1)
- **Markdown rendering** for caveats: `renderCaveatsMarkdown()` library API +
  `--markdown` CLI flag, mirroring how `report` is to `classify`. (#24)
- **`scan` integration:** `mcp-recon scan` emits `caveats.json` as a 5th
  artifact when binding flags are provided. No-binding case unchanged. (#25)

### Added — Dataset + findings

- **Public dataset expanded from 4 to 9 servers** — added scan artefacts for
  `server-fetch`, `server-git`, `server-postgres`, `server-puppeteer`, and
  `server-time` alongside the existing filesystem / everything / memory /
  sequential-thinking. Total: 60 tools classified, 2,761 fuzz calls. (#12)
- **First findings corpus** at `findings/` — 6 writeups (F001–F006) covering
  capability-surface observations, a real DoS amplification on
  `server-puppeteer`, a navigate-allowlist bypass on its `puppeteer_evaluate`,
  and a positive-control writeup of `mcp-server-git`'s `--repository`
  allowlist pattern. (#12)

### Added — Quality + tooling

- **GitHub Actions CI** — Node 20+22 matrix (typecheck + lint + test), Rust
  stable (fmt + clippy `-D warnings` + workspace test), dataset-validate
  (jq walk of `examples/public-servers/*/{inventory,classification,fuzz}.json`),
  PR-title conventional-commits gate. Plus monthly Dependabot for npm + cargo
  + GitHub Actions. (#7)
- **Performance benchmarks** via `tinybench` — `npm run bench` benches
  `classify` / `classify+fuzz-fold` / `renderMarkdown` / `planCaveats` against
  the four published reference servers + a synthetic-scale sweep at
  10/100/1 000/10 000 tools. Confirms `classify` < 1 s at all sizes. Bench
  workflow posts results as a PR comment. (#10)
- **JSON Schema files** at `schemas/` — formal `inventory.v0.1.json`,
  `fuzz.v0.1.json`, `classification.v0.1.json`, `caveats.v0.1.json` with ajv
  validation tests. Stable contract for downstream consumers. (#9)
- **Adversarial-server fixture harness** — 8 stdio-MCP fixtures simulating
  attacker behaviors (10 k tool list, 10 MB descriptions, recursive `$ref`,
  malformed UTF-8, slow response, ANSI/BEL bytes, prompt-injection-shaped
  descriptions, schema violations). Found and fixed two real bugs in
  `enumerate.ts`: missing per-call timeout, unbounded tool descriptions. (#11)

### Added — Docs + governance

- **`SECURITY.md`** + `SECURITY-INSIGHTS.yml` + `CITATION.cff`. Establishes
  vulnerability-reporting policy and separates "vulns IN mcp-recon" from
  "vulns found USING mcp-recon" with explicit disclosure paths. (#22)
- **`CONTRIBUTING.md`** + issue/PR templates (bug, classifier-finding,
  feature-request) + PR template + `config.yml` linking to security
  advisories and Discussions. (#6)
- **`README.md`** polish — badge row, table of contents, command cheatsheet,
  comparison table vs garak / Burp / manual review, recon → capnagent
  pipeline diagram. (#8)
- **Initial CHANGELOG.md** following Keep a Changelog v1.1.0. (#26)

### Fixed

- **F006 — classifier mis-tagged IANA timezone strings as filesystem paths.**
  `looksPathy()` matched `source_timezone`/`target_timezone` because the
  path-name list contained `source` and `target`. Stop-word check added.
  7 regression tests pin the fix. (#27)
- **AND-split was quote-unaware** in `parseRecommendedCaveat()`. A tool
  name containing `AND` would fragment predicates. Replaced with a
  quote-aware `splitOnUnquotedAnd()` walker. 13 adversarial-input regression
  tests pin the fix. (#1, #23)
- **Bench workflow** lacked `pull-requests: write` permission, blocking the
  PR-comment step. Added at job level. (#10)

### Changed

- **Version:** `packages/mcp-recon-cli/package.json` 0.0.1 → 0.2.0. (#26)
- **`docs/SPEC.md`** — v0.1 surface now lists six commands (was five). (#1)
- **`README.md`** — new "From recon to a capnagent issuer in one pipe"
  section showing the recon → capnagent handoff. (#1)

### Dependency updates (Dependabot)

- `actions/checkout` 4 → 6 (#13)
- `actions/cache` 4 → 5 (#14)
- `amannn/action-semantic-pull-request` 5 → 6 (#15)
- `actions/upload-artifact` 4 → 7 (#16)
- `actions/setup-node` 4 → 6 (#17)
- `@types/node` 20.16.11 → 25.6.0 (#18)

### Deferred (not in this release)

- `@biomejs/biome` 1 → 2, `vitest` 2 → 4, `typescript` 5 → 6 (open as Dependabot PRs)
- Re-scan of `examples/public-servers/server-time/` against the post-F006 classifier

## [0.1.2] — 2026-04-30

### Fixed

- Pre-launch fixes: TS build, stale README, WRITEUP prose drift.

## [0.1.1] — 2026-04-29

### Changed

- Rerun dataset at default `N=200` budget. DoS finding strengthens
  (1 → 7 `runtime_errors`).

## [0.1.0] — 2026-04

Initial public release. Compresses several pre-tag development weeks into a
single milestone — the v0.1 ship date corresponds to the `scanned_at` field
of the `examples/public-servers/server-*/classification.json` artefacts.

### Added

- `mcp-recon scan` end-to-end command, dataset migration, writeup, and
  social preview (v0.1 ship).
- Week 3: classifier + Markdown reporter — four public-server threat
  profiles.
- Week 2: schema-aware fuzzer (six axes, deterministic PRNG; 489 calls
  across four public servers).
- Week 1: `enumerate` command end-to-end + integration test +
  `METHODOLOGY.md` + initial inventories.

[Unreleased]: https://github.com/euanmcrosson-dotcom/mcp-recon/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/euanmcrosson-dotcom/mcp-recon/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/euanmcrosson-dotcom/mcp-recon/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/euanmcrosson-dotcom/mcp-recon/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/euanmcrosson-dotcom/mcp-recon/releases/tag/v0.1.0
