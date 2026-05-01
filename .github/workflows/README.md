# GitHub Actions

This directory contains the CI workflows for `mcp-recon`.

## `ci.yml`

Runs on every `push` to `master`/`main` and on every `pull_request`.

In-flight runs for the same ref are cancelled when a newer commit lands
(`concurrency.cancel-in-progress: true`), so PRs only ever burn one set of
runners at a time.

### Jobs

| Job                | Purpose                                                                                   | Gates merge? |
| ------------------ | ----------------------------------------------------------------------------------------- | ------------ |
| `test`             | Node 20 + 22 matrix. Runs `npm ci`, `typecheck`, `lint` (informational), `test`.          | yes (test)   |
| `rust-check`       | Stable Rust. Runs `cargo fmt --check` (informational), `cargo clippy -D warnings`, `cargo test --workspace`. | yes (clippy + test) |
| `dataset-validate` | jq-based check that every `examples/public-servers/*/{inventory,classification,fuzz}.json` parses and carries the expected `mcp-recon/v0.1/<kind>` schema tag. Does **not** run mcp-recon — no MCP server is spun up in CI. | yes |
| `pr-titles`        | Lints PR titles against a conventional-commits allowlist (`feat`, `fix`, `chore`, `test`, `docs`, `refactor`, `build`, `ci`, `perf`, `style`, `revert`). Only runs on `pull_request` events. | yes (PRs)    |

### Why `lint` and `cargo fmt --check` are `continue-on-error`

The repo has pre-existing Biome diagnostics (58 at the time of this CI
bringup) and one rustfmt import-order nit. Those are tracked separately
and would be fixed in dedicated PRs — gating *this* CI on them would just
freeze master. The steps still run on every commit, so the warning shows
up in the run summary without painting the whole job red. Once the
backlog is cleared, drop `continue-on-error: true` to convert them into
hard gates.

### Caching

- `actions/setup-node@v4` with `cache: 'npm'` — keyed on `package-lock.json`.
- `actions/cache@v4` for `~/.cargo/{registry,git}` and `target/` — keyed on
  `Cargo.lock`.

### Test artifact upload

The final `test` step uploads anything matching
`packages/*/{coverage,test-results}/**` or `packages/*/junit.xml` as
`test-results-node<version>` (14-day retention). It is best-effort —
`if-no-files-found: ignore` and `continue-on-error: true` so a missing
report folder never paints the build red. If/when a workspace starts
emitting JUnit XML or `coverage/`, the artifact will appear automatically.

## Debugging failing runs

1. **Open the run in the Actions tab** — start with the failing job's last
   step. Each step's logs are available there for 90 days.
2. **Reproduce locally**:
   - Node: `npm ci && npm run --workspaces --if-present typecheck && npm run --workspaces --if-present test`
   - Rust: `cargo clippy --all-targets --workspace -- -D warnings && cargo test --workspace`
   - Dataset: run the bash block from the `dataset-validate` job — it
     uses only `jq` and `bash`, no project state.
3. **Re-running**: use the **Re-run failed jobs** button on the run
   summary. The cargo + npm caches usually make a re-run finish in
   under a minute.
4. **PR title fails `pr-titles`?** Edit the PR title to start with one of
   the allowed types (`feat:`, `fix:`, `chore:`, ...). The job re-runs
   automatically when the title changes.
5. **`dataset-validate` schema mismatch?** The error annotation points
   at the offending file and shows the expected vs. actual `schema`
   value. Regenerate that artifact from the CLI rather than hand-editing
   the JSON.

## Pinning policy

All third-party actions are pinned to a major tag (e.g.
`actions/checkout@v4`, `dtolnay/rust-toolchain@stable`,
`amannn/action-semantic-pull-request@v5`). `dependabot.yml` keeps those
tags fresh on a monthly cadence — review the bumps in the auto-opened
PRs before merging.
