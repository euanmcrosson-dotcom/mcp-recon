# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] — 2026-05-01

### Added

- `mcp-recon caveats <classification.json> [--caller] [--sandbox-prefix] [--expiry]` —
  sixth subcommand. Consumes `mcp-recon/v0.1/classification` and emits a
  `mcp-recon/v0.1/caveats` document of capnagent-ready issuance plans, with
  placeholder substitution and structured flags (`classification_unknown`,
  `low_confidence`, `cdc_without_arg_constraint`, `unsubstituted_placeholder`).
  Closes the manual-paste loop between recon and capnagent. (PR #1)
- `planCaveats(classification, bindings)` library API plus types
  (`CaveatBindings`, `CaveatPlan`, `CaveatsResults`, `FlagReason`,
  `CAVEATS_SCHEMA`).
- 24 new vitest tests covering caveats placeholder substitution, flag
  emission, and end-to-end planning.

### Changed

- `docs/SPEC.md` — v0.1 surface now lists six commands (was five).
- `README.md` — new "From recon to a capnagent issuer in one pipe" section
  showing the recon → capnagent handoff.

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
