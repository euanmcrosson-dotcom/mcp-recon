# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
