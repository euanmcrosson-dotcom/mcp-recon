# F006 — Classifier mis-tags IANA timezone strings as filesystem paths

| Field | Value |
|---|---|
| **Component** | mcp-recon classifier (`packages/mcp-recon-cli/src/classify/rules.ts`) |
| **Triggering server** | `mcp-server-time` v1.27.0 (PyPI `mcp-server-time==2026.1.26`) |
| **Triggering tool** | `convert_time` (also affects `get_current_time` minorly) |
| **Discovery date** | 2026-05-01 |
| **Severity** | low |
| **Severity rationale** | Real bug in mcp-recon, not in any external server. The classifier's "path-shaped argument" heuristic fires on any string that contains a forward slash and produces a `data_class=filesystem` rationale that is plainly wrong for IANA timezone names like `America/New_York`. The output caveat instructs the operator to bind the argument to a `<your-sandbox-prefix>/` filesystem prefix — which would break the tool's correct usage. Severity is low because the false-positive rationale is human-readable in the report.md and the operator will catch it before deployment, but the misclassification still pollutes the dataset and any downstream automation that consumes the JSON. |
| **Coordinated disclosure status** | open (fix lands in this repo as a follow-up PR) |

## Description

The classifier rule in question is the path-shape heuristic:

```ts
// packages/mcp-recon-cli/src/classify/rules.ts (paraphrased)
if (typeof arg.value === "string" && /[/\\]/.test(arg.value)) {
  // → data_class hint: filesystem (weight 0.40)
}
```

The intent is to detect path-shaped arguments like `/tmp/data` or
`C:\Users\...`. The heuristic does not exclude valid non-path
strings that happen to contain slashes — IANA timezone IDs being
the canonical example.

For `mcp-server-time.convert_time`, both `source_timezone` and
`target_timezone` are documented as IANA names. The classifier
sees the slash in `America/New_York`, `Europe/London`, etc., and
emits:

```json
{
  "tool": "convert_time",
  "data_class": "filesystem",
  "authority_level": "read",
  "confidence": 0.64,
  "rationale": "schema: arg \"source_timezone\" is path-shaped → filesystem (0.40); schema: arg \"target_timezone\" is path-shaped → filesystem (0.40)",
  "recommended_caveat": "tool == \"convert_time\" AND ... AND arg.source_timezone starts_with \"<your-sandbox-prefix>/\" AND arg.target_timezone starts_with \"<your-sandbox-prefix>/\" ..."
}
```

A capnagent operator who pasted that caveat unmodified would
deny every legitimate `convert_time` call (no sandbox prefix
matches `America/New_York`).

The same heuristic also produced a path-shaped hit on the URL
arguments in `server-puppeteer` and `server-fetch`, but in those
cases the data class was already `network` for unrelated reasons
(URL-shaped argument, name match), so the path-shaped vote got
dominated and didn't surface in the output. `convert_time` is
the corner case where path-shaped is the *only* signal, so the
classifier confidently lands at the wrong class.

## Reproduction

```bash
# pip install mcp-server-time
mcp-recon scan "stdio:python -m mcp_server_time" \
  --out=examples/public-servers/server-time --budget=200

# Inspect the offending classification:
node -e '
console.log(
  require("./examples/public-servers/server-time/classification.json")
    .classifications.find(c => c.tool === "convert_time")
);
'
```

Expected: a `data_class: "filesystem"` field with the path-shaped
rationale cited above.

## Suggested fix

Three options, in order of effort:

1. **Quick fix:** Add a stop-word list to the path-shape
   heuristic. If the argument name contains `timezone`, `tz`,
   `zone`, `region`, `locale`, *or* the schema's enum lists IANA
   names explicitly, suppress the path-shape vote.
2. **Better fix:** Tighten the regex. `[/\\]` is too permissive;
   require either a leading `/`, a leading drive letter
   (`[A-Za-z]:\\`), or `..` segments before treating the string
   as path-shaped.
3. **Best fix:** Lower the rule's weight from 0.40 to ~0.20 and
   require corroborating evidence (path-shaped arg name, e.g.,
   `path`, `dir`, `file`, `repo_path`) before promoting to a
   filesystem classification. This is consistent with the
   classifier's noisy-OR design — single-signal classification
   should be lower-confidence than multi-signal.

Option 1 is the smallest patch; option 3 is the right
architectural fix and what the v0.2 classifier rework should
do anyway.

## Severity rationale

This is "low" rather than "informational" because:

- The misclassified caveat is *also* in `classification.json` as a
  machine-readable artefact, not just `report.md`. Any pipeline
  that consumes `classification.json` and emits capnagent issuance
  plans without human review will break the tool.
- The classifier's confidence on this output is 0.64 — the same
  band as a real filesystem-shaped argument — so a downstream
  scoring rule that filters by confidence won't catch it.

It's not "medium" because the fix is local to one rule, the
output is human-readable enough that an operator reviewing the
report.md will spot it, and `mcp-server-time` has a small enough
surface that the blast radius is one tool.

## Status

- 2026-05-01: Filed as F006.
- Fix queued for the v0.2 classifier rework PR. Until then,
  `mcp-server-time/classification.json` ships with this known
  false positive and the report.md includes a caveat noting it.
