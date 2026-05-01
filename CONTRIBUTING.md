# Contributing to mcp-recon

Thanks for your interest in `mcp-recon`. This document is the entry point
for anyone who wants to contribute code, classifier rules, fuzz axes, or
findings against real-world MCP servers.

## Welcome

`mcp-recon` is a **security-research tool** for reverse-engineering MCP
server tool surfaces — enumerate, fuzz, classify, report. It is the
offensive companion to `capnagent` (the defensive side). The audience is:

- **Security researchers** auditing MCP servers in the wild — the
  classifier output is meant to surface confused-deputy candidates and
  ambient-authority leaks worth investigating manually.
- **MCP server developers** who want to know what their server's tool
  surface looks like to a hostile classifier before shipping.
- **`capnagent` integrators** wiring caveat-suggestion into agents — the
  per-tool caveat in the threat profile feeds directly into capnagent.

### Scope

`mcp-recon` has a deliberately small scope. See
[`docs/SPEC.md`](docs/SPEC.md) for the full v0.1 spec and **non-goals**.
Highlights of what's explicitly out of scope:

- No replay attacks, proxy mode, or active exploitation.
- No LLM-in-the-loop (v0.1 is deterministic and rule-based).
- No persistence layer, no GUI, no auth shimming.
- No multi-server cross-product fuzzing (that's the round-01 shape).

Before opening a feature request, please check `docs/SPEC.md` so we don't
spend cycles debating something that is non-goal by design.

## Quick start (dev environment)

Requirements: Node.js >= 20, npm, and Rust (only if you're touching the
`crates/` workspace).

```bash
git clone https://github.com/euanmcrosson-dotcom/mcp-recon.git
cd mcp-recon
npm install
npm test          # runs the CLI test suite
npm run typecheck # TypeScript typecheck across workspaces
npm run build     # build all workspaces
```

If you're touching Rust:

```bash
cargo test
cargo build
```

Run the CLI locally:

```bash
npm run recon -- enumerate stdio:npx @modelcontextprotocol/server-filesystem /tmp
```

## How to add a classifier rule

Classifier rules live in
[`packages/mcp-recon-cli/src/classify/rules.ts`](packages/mcp-recon-cli/src/classify/rules.ts).
This file is the single source of truth the classifier reads.

Each rule is a `NameOrDescRule`:

```ts
{
  pattern: RegExp,            // case-insensitive
  data_class: DataClass,      // filesystem | network | shell | payments |
                              // messaging | system | metadata | ...
  authority_floor: AuthorityLevel, // read | write | destructive | privileged
  scope: "name" | "description" | "either",
}
```

Confidence weights (all defined at the top of `rules.ts`):

- Tool **name** match — `0.7` (strong cue, gameable)
- Tool **description** match — `0.5`
- **Schema** match — `0.4`
- Side-effect verb in description — escalates authority by one step
- `FUZZ_INFORMED_BONUS` — `0.1` when fuzz results show the tool actually
  accepts adversarial inputs.

### Worked example: adding a `cryptocurrency` data class

Suppose you want to flag tools that move crypto funds — `send_eth`,
`transfer_btc`, `swap_tokens`, etc. — because the blast radius is high
and they're confused-deputy candidates by default.

1. **Add the data class.** In
   `packages/mcp-recon-cli/src/classify/types.ts`, add `"cryptocurrency"`
   to the `DataClass` union. Run `npm run typecheck` — you'll get
   exhaustiveness errors in any switch over `DataClass`. Fix them.

2. **Add the rules.** In `rules.ts`, append (in the appropriate section,
   ordered most-specific to most-general):

   ```ts
   // ─── cryptocurrency ─────────────────────────────────────────
   {
     pattern: /\b(send|transfer|swap)[_-]?(eth|btc|sol|tokens?|crypto)\b/i,
     data_class: "cryptocurrency",
     authority_floor: "destructive", // moves funds, can't undo
     scope: "name",
   },
   {
     pattern: /\b(wallet|blockchain|onchain|defi|stablecoin|gas[_-]?fee)\b/i,
     data_class: "cryptocurrency",
     authority_floor: "write",
     scope: "description",
   },
   ```

3. **Update `docs/METHODOLOGY.md`** with a change-log entry: date,
   reason, what existing tools the rule now flags. The rules file's
   header comment (`// Adding a rule? Update this file AND docs/...`) is
   load-bearing — please don't skip this step.

4. **Add a test fixture.** Put a representative tool inventory in
   `packages/mcp-recon-cli/src/__tests__/` and assert the new
   `data_class` is emitted with the expected authority.

5. **Re-run the dataset.** Run `npm test` and the dataset validator. If
   the new rule changes any classification in `examples/public-servers/`,
   commit the regenerated reports as part of the same PR.

## How to add a fuzz axis

Fuzz axes live in
[`packages/mcp-recon-cli/src/fuzz/axes/`](packages/mcp-recon-cli/src/fuzz/axes/).
The 6 existing axes are documented in
[`docs/METHODOLOGY.md`](docs/METHODOLOGY.md):

- `boundary.ts` — empty/max strings, 0/-1, `Number.MAX_SAFE_INTEGER + 1`,
  `NaN`, `null`
- `type-confusion.ts` — string when number expected, etc.
- `encoding.ts` — percent-encoding, Unicode homographs, null bytes,
  backslash escapes
- `path-traversal.ts` — `../`, `%2e%2e/`, Windows `\\`-separators on
  POSIX
- `url-hostility.ts` — userinfo splitting, IDN homograph, scheme
  tricks, port tricks
- `schema-violation.ts` — extra/missing/wrong-typed fields in nested
  structures

To add an axis:

1. Create a new file in `packages/mcp-recon-cli/src/fuzz/axes/`
   (e.g. `time-skew.ts`).
2. Implement the axis interface in `../types.ts` — it must take a
   JSON Schema and yield a deterministic stream of inputs given a
   seeded PRNG (see `../prng.ts`). **Determinism is load-bearing**:
   re-running with the same seed must produce bit-identical inputs.
3. Wire it into `../index.ts` so it's part of the default rotation.
4. Add a budget allocation in `../index.ts` — every axis gets a slice
   of the per-tool budget; document the rationale.
5. Update `docs/METHODOLOGY.md` with a §"Fuzz axes" entry.
6. Add a test that asserts determinism: same seed + same schema =
   same byte-for-byte input list.

## How to add a corpus round / publish a finding

If you've used `mcp-recon` against a real-world MCP server and found
something interesting (a confused-deputy pattern, a misclassified tool,
an authority leak), here's the workflow:

1. **Scan the server.** Run `mcp-recon scan stdio:...` (or the http:
   form). Save the JSON outputs *and* the Markdown report.
2. **Triage the findings.** Read the classification + report. If the
   classifier missed something or flagged something it shouldn't have,
   open a `classifier_finding` issue (see issue templates) so we can fix
   the rule table.
3. **Coordinate with the maintainer of the scanned server.** This
   project follows responsible disclosure for findings against
   third-party servers. Email the maintainer privately first, give them
   reasonable time to respond, then publish. The default coordination
   window is 30 days unless the issue is publicly exploitable in which
   case a shorter window is appropriate.
4. **Propose a public-servers entry.** Add a directory under
   `examples/public-servers/<server-name>/` containing:
   - `inventory.json` — the raw enumeration output.
   - `fuzz.json` — the fuzz output (with the seed pinned).
   - `classification.json` — the classifier output.
   - `report.md` — the threat profile.
   - `NOTES.md` — your analysis: what's interesting, what `capnagent`
     caveats you'd recommend, any maintainer-coordination notes (date
     contacted, response, fix timeline).
5. **Open a PR.** Use the conventional commit prefix `docs:` or `feat:`
   depending on whether the entry forces any classifier-rule changes.

## Conventional commits

Every commit on a PR must follow [Conventional
Commits](https://www.conventionalcommits.org/):

- `feat:` — new feature (new CLI command, new fuzz axis, new rule
  category)
- `fix:` — bug fix
- `chore:` — tooling, deps, non-functional cleanup
- `test:` — tests only
- `docs:` — documentation only
- `refactor:` — refactor without behavior change
- `build:` — build-system / packaging changes
- `ci:` — CI config changes

Squash-merge is the default — your PR's title becomes the squash commit
message, so make the PR title itself a valid conventional-commit line.

## PR review process

This is currently a solo project but the bar is the same as any other
PR. Every PR must:

- [ ] Have all tests passing (`npm test` and `cargo test` if Rust
      touched).
- [ ] Have `npm run typecheck` clean.
- [ ] Have the dataset validator clean (no drift in
      `examples/public-servers/` reports unless the PR explicitly
      changes classifier behavior, in which case the regen is part of
      the PR).
- [ ] Have a conventional-commit-style title.
- [ ] Fill out the PR template (Summary, Linked issue, Test plan, Risk,
      Backwards compat).

For classifier-rule changes specifically, the reviewer will look for the
`docs/METHODOLOGY.md` change-log entry. No silent rule drift.

## Releasing

Versions follow [SemVer](https://semver.org/) and are tagged `vX.Y.Z`.

1. Bump the version in `package.json` and any per-workspace
   `package.json` that has its own version.
2. Add a `CHANGELOG.md` entry under a new `## [X.Y.Z] — YYYY-MM-DD`
   header. Group changes under `Added`, `Changed`, `Fixed`, `Removed`.
3. Commit with `chore(release): vX.Y.Z`.
4. Tag: `git tag -a vX.Y.Z -m "vX.Y.Z"`.
5. Push the tag: `git push origin vX.Y.Z`.
6. **NPM publish** — applicable once the CLI ships to npm. The current
   v0.1 line is not yet published; when it is, the release script will
   run `npm publish --workspace @mcp-recon/cli --access=public` from a
   clean tag.
7. Cut a GitHub Release pointing at the tag, with the CHANGELOG entry
   as the release notes.

## Questions?

Open a GitHub Discussion for general questions, or a security advisory
(private) for anything sensitive. The contact links at issue-creation
time route both correctly.
