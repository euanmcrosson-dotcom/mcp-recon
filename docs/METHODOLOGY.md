# mcp-recon methodology

> **Status banner (added 2026-05-27):** the classifier described here is
> shipped (7 rules, see the [README](../README.md)). The fuzzer is **not yet
> built** — treat fuzzer sections as design intent, not current behaviour.

This document describes how mcp-recon's classifier and fuzzer
work, what signals they consume, and what would falsify each
classification. It's written **before** the week-3 implementation
ships so the design isn't bent to fit observations after the fact.

If the implementation diverges from this document, the divergence
is a bug — either fix the code to match, or update the doc with
explicit rationale and a commit reference. No silent drift.

> **Honest framing.** mcp-recon is not authoritative. It produces
> a *structured second opinion* about what an MCP server's tool
> surface implies. A senior security reviewer should be able to
> read the report, disagree with any classification, and have an
> obvious path to override it (the `rationale` field on every
> classification names the rules that fired).

---

## Inputs

The classifier consumes one or more of:

1. **Tool inventory** (`mcp-recon/v0.1/inventory`) — the structured
   output of `mcp-recon enumerate`. Required.
2. **Fuzz results** (`mcp-recon/v0.1/fuzz`) — output of `mcp-recon
   fuzz`. Optional in v0.1; raises classifier confidence when
   present.
3. **Operator hints** (future, v0.2) — overrides for
   classifications the operator disagrees with. Not in v0.1.

The classifier MUST work with input #1 alone. Fuzz results are a
second-pass refinement, not a prerequisite.

---

## Signals the classifier reads

For each tool the classifier examines:

| Signal | Source field | What it tells us |
|---|---|---|
| **Tool name** | `tool.name` | Verb + object: `read_file`, `delete_path`, `create_entities`. Strong cue but gameable. |
| **Description** | `tool.description` | Free-form prose. Strong cue when present; some servers ship empty descriptions. |
| **Input schema** | `tool.inputSchema` | The argument shapes and types. Reveals whether the tool takes paths, URLs, command arrays, etc. |
| **Side-effect verbs** in description | `tool.description` regex | "writes", "deletes", "sends", "executes" — the destructive vocabulary. |
| **User-controllable string args** | `tool.inputSchema` walk | Any `type: string` arg without an enum constraint is gameable from a prompt-injection perspective. |
| **Required-args presence** | `tool.inputSchema.required` | An args-required tool has *intent*; an args-optional tool may be a config gate. |

Signals NOT used by v0.1:

- Server name / version (helps in v0.2 for cross-server reasoning).
- Tool annotations / categories from the MCP spec (the spec has
  `readOnlyHint` etc., but adoption is low; classifier shouldn't
  rely on operators having set them).
- LLM-generated descriptions (v0.2 adds an LLM as a secondary
  classifier; v0.1 is rules-only).

---

## The rule taxonomy

Each rule is a **(name-or-description-pattern → data-class +
authority-level)** triple, plus a confidence weight in
`[0.0, 1.0]`. The classifier accumulates evidence and reports the
highest-confidence assignment.

### Data classes (7 + Unknown)

| Class | Strong-evidence signals |
|---|---|
| `Filesystem` | `read_file`, `write_file`, `directory_tree`, `list_directory`, `move_file`, `delete_path`, `path` arg |
| `Network` | `fetch`, `http`, `get`, `post`, `url` arg, `origin` arg |
| `Shell` | `exec`, `shell`, `spawn`, `run`, `command` arg, `args` array arg |
| `Payments` | `charge`, `refund`, `purchase`, `wire`, `amount` arg with currency unit |
| `Messaging` | `send`, `email`, `notify`, `message`, `to` / `recipient` arg |
| `System` | `env`, `process`, `getenv`, `system_info`, no-arg getters |
| `Metadata` | `get_*_info`, `list_allowed_*`, `read_graph`, `search_*` (read-only metadata) |
| `Unknown` | No matching pattern. The classifier emits this rather than guessing. |

### Authority levels (4)

| Level | Definition |
|---|---|
| `Read` | Tool reads bounded data; cannot mutate state. `list_directory`, `read_file`, `get_*`. |
| `Write` | Tool mutates state but reversibly. `write_file`, `create_entities`, `add_observations`. |
| `Destructive` | Tool removes state irreversibly. `delete_*`, `drop_*`, `rm_*`. |
| `Privileged` | Tool spawns subprocesses or otherwise hands authority to a child. `exec`, `shell`, `run`. |

### The confused-deputy flag

A tool is flagged `confused_deputy_candidate=true` if **both**:

1. It has at least one user-controllable `string` arg (not enum-bounded).
2. Its authority-level is `Write`, `Destructive`, or `Privileged`.

This is the load-bearing signal. It identifies the tools where
prompt injection has the highest leverage — capnagent's round-01
shape, generalised.

False-positive rate matters less than false-negative rate here. We
prefer to over-flag; the human reviewer triages.

---

## Confidence scoring

Each rule's match contributes to the tool's overall confidence:

- **Tool name match** — confidence 0.7 (strong cue, gameable)
- **Description match** — confidence 0.5 (helpful, often missing)
- **Schema match** (e.g. arg named `path` with `type: string`) — confidence 0.4 (structural; gameable too)
- **Side-effect verb in description** — confidence 0.6 (raises authority-level by one step)

Combined confidence: `1 - ∏(1 - c_i)` over matched rules (the
"noisy-OR" combination — standard for independent evidence). The
`Unknown` data-class fires only if no rule above 0.3 matched.

The classifier output records:

- The *winning* data-class + authority-level
- The combined confidence
- A free-form `rationale` listing every rule that fired with its
  individual confidence

The `rationale` is the load-bearing field for human review.

---

## What would falsify a classification

For each output, here's what a senior reviewer could check:

| Field | Falsifying observation |
|---|---|
| `data_class` | A tool description that obviously belongs to a different class. The reviewer can override by editing the rule file. |
| `authority_level` | A tool the classifier marks `Read` that the reviewer demonstrates can mutate state (e.g. running it twice and observing different observable side effects). |
| `confused_deputy_candidate` | A tool flagged as such where the reviewer demonstrates the string args are sanitised at the server side. (Note: server-side sanitisation is opaque to mcp-recon by design — the flag is about *capability*, not *vulnerability*.) |
| `confidence` | Two reviewers disagree on the right rule weights — the rule file is the source of truth, and disagreement leads to a change in the rule weights, not in any individual classification. |

The rule file (`crates/mcp-recon-core/src/rules/v0_1.rs` once the
implementation lands) is itself the falsifiable artifact: every
rule has a name, a pattern, a weight, and a comment explaining
why. A rule that reviewers consistently override is a rule that
needs its weight adjusted.

---

## Fuzzer methodology

Per docs/SPEC.md §"Fuzzing strategy", v0.1 fuzzes along six axes
with a deterministic PRNG:

| Axis | Generator | Bounds |
|---|---|---|
| Boundary values | Empty string, max-length string, 0, -1, MAX_SAFE_INT+1, NaN, null | Per JSON Schema type |
| Type confusion | Wrong type for each declared field | One per field |
| Encoding tricks | Percent-encoded, Unicode-homograph, null-byte, backslash-escaped | For string args |
| Path traversal | `../`, `%2e%2e/`, mixed separators, absolute paths | For path-shaped args (heuristic: arg name contains "path") |
| URL hostility | Userinfo splitting, IDN homograph, scheme tricks, port tricks | For URL-shaped args (heuristic: arg name contains "url" or "origin") |
| Schema violation | Extra fields, missing required, wrong types in nested | Per schema |

Default budget: 200 calls per tool. PRNG seed: `0xC0FFEE`. Outputs
are recorded but not classified — the human (or v0.2 ML
classifier) interprets which fuzz inputs revealed something
interesting.

---

## What mcp-recon explicitly does NOT measure

Honest about the boundaries:

- **Server-side correctness.** The server might be vulnerable in
  ways we can't see from the client. mcp-recon classifies
  *exposed authority*, not *exploitability*.
- **Cross-server interactions.** A tool from server A might be
  weaponized by an injection from server B (the round-01
  cross-server confused-deputy class). v0.1 classifies one
  server at a time.
- **Auth bypass.** If the server requires auth and we have a
  pre-authenticated transport, we see the authenticated tool
  surface. We don't try unauthenticated paths.
- **Behavioural side-effects.** A tool named `read_file` might
  have hidden write side effects in a malicious server. The
  classifier reads the *declared* surface; behaviour-based
  analysis is a different category of tool.

---

## Methodology change-log

When the methodology changes, that's a meaningful event. Append
here with date + reason + commit hash. v0.1 starts the log.

| Date | Change | Reason | Commit |
|---|---|---|---|
| (v0.1 ship date) | Initial methodology | Document the v0.1 design | (TBD) |

---

## How this document gets used

- **Pre-week-3:** the implementation reads this document and builds the rule file to match.
- **During implementation:** if the code wants to add a new signal not listed in §"Signals," update this document first.
- **Post-implementation:** any classification a reviewer disputes goes back to this document — does the methodology say something that turned out to be wrong, or does the implementation diverge from the methodology?
- **For the v0.1 writeup:** the writeup links here for "how does it work?" — readers shouldn't have to read the code to understand the methodology.
