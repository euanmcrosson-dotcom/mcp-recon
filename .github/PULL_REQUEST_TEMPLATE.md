<!--
PR title must follow Conventional Commits — it becomes the squash-commit
message. Examples:
  feat(classify): add cryptocurrency data class
  fix(fuzz): respect seed across boundary axis
  docs: document the fuzz-axis contract
-->

## Summary

<!-- 1-3 sentences. What does this PR do, and why? -->

## Linked issue

<!-- "Closes #123" / "Refs #45". Use "N/A" if there is no issue. -->

Closes #

## Test plan

<!-- Check every box that applies; add custom rows if you ran extra checks. -->

- [ ] `npm test` passes locally
- [ ] `npm run typecheck` is clean
- [ ] `cargo test` passes (only if Rust crates were touched)
- [ ] Dataset under `examples/public-servers/` validates (regenerated +
      committed if classifier behavior changed)
- [ ] Manual smoke test: `mcp-recon scan stdio:...` against at least one
      server still produces a valid report
- [ ] New tests added for new behavior (or rationale for "no test
      needed" stated below)

<!-- If you added new tests, list them here. -->

## Risk

<!-- Pick one and justify in 1-2 sentences. -->

- [ ] **Low** — docs / tests / non-functional refactor / new code path
      gated behind an opt-in flag.
- [ ] **Medium** — touches existing classifier rules, fuzz axes, or CLI
      output. Could change downstream consumer behavior.
- [ ] **High** — changes JSON output schema, breaks a public CLI flag,
      or rewrites a load-bearing module.

**Reasoning:**

<!-- One paragraph: blast radius if this is wrong, what mitigates it. -->

## Backwards compatibility

- [ ] **Not a breaking change** — JSON schemas, CLI flags, exit codes,
      and the `examples/public-servers/` dataset are all unchanged
      (or only additive).
- [ ] **Breaking change** — see notes below.

<!-- If breaking, describe: which schema/flag/contract changed, what the
     migration is, whether the SemVer bump should be major. -->

## Conventional-commit-style title

<!-- Confirm the PR title matches one of:
       feat(scope): ... | fix(scope): ... | chore: ... | test: ...
       docs: ... | refactor: ... | build: ... | ci: ...
     The title becomes the squash commit message. -->

- [ ] PR title is a valid conventional-commit line.
