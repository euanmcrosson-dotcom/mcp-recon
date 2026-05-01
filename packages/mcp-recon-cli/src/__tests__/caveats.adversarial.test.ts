/**
 * Adversarial-input robustness corpus for `planCaveats()`.
 *
 * Threat model: a malicious upstream MCP server controls the tool
 * names, descriptions, and (transitively) the `recommended_caveat`
 * strings produced by `mcp-recon classify`. If `planCaveats()`
 * mishandles these strings, the operator's downstream capnagent
 * issuer ingests broken or attacker-influenced caveats. Each case
 * below probes one plausible mishandling and pins the documented
 * defense.
 *
 * Every payload is SYNTHETIC — no real attack strings.
 */

import { describe, expect, it } from "vitest";

import { planCaveats } from "../caveats/index.js";
import type { CaveatBindings } from "../caveats/types.js";
import { CLASSIFICATION_SCHEMA } from "../classify/index.js";
import type {
  Classification,
  ClassificationResults,
} from "../classify/types.js";

const FROZEN_NOW = "2026-04-29T12:00:00.000Z";

const FULL_BINDINGS: CaveatBindings = {
  caller: "agent:planner",
  sandbox_prefix: "/var/sandbox/tenant-1",
  expiry: "2026-05-01T18:00:00.000Z",
};

/** Build a single-classification document with one tool entry. */
function makeClassification(entry: Classification): ClassificationResults {
  return {
    schema: CLASSIFICATION_SCHEMA,
    scanned_at: FROZEN_NOW,
    server: { name: "adversarial-server", version: "0.0.0" },
    fuzz_informed: false,
    classifications: [entry],
  };
}

/** Default-ish classification entry — callers override what they need. */
function entry(overrides: Partial<Classification>): Classification {
  return {
    tool: "t",
    data_class: "metadata",
    authority_level: "read",
    confused_deputy_candidate: false,
    confidence: 0.9,
    rationale: "synthetic adversarial fixture",
    recommended_caveat: 'tool == "t" AND caller == "<your-caller-id>"',
    ...overrides,
  };
}

describe("planCaveats — adversarial: AND keyword in identifiers/values", () => {
  // Case 1. Attack: malicious server names a tool `foo AND bar`. The
  // classifier emits `tool == "foo AND bar" AND ...`. A naive
  // `split(/\s+AND\s+/)` splits the tool predicate in half, producing
  // a malformed `tool == "foo` caveat that capnagent would either
  // reject or — worse — accept as a wildcard if its parser is lenient.
  // Defense: quote-aware splitter preserves the literal.
  it("preserves tool predicate when tool name contains the word AND (not split mid-string)", () => {
    const c = makeClassification(
      entry({
        tool: "foo AND bar",
        recommended_caveat:
          'tool == "foo AND bar" AND caller == "<your-caller-id>" AND now <= @<your-cap-expiry>',
      }),
    );
    const out = planCaveats(c, FULL_BINDINGS);
    const plan = out.plans[0]!;
    // Defense proven: the tool predicate appears as a single, intact caveat.
    expect(plan.caveats).toContain('tool == "foo AND bar"');
    // No fragmented "tool == \"foo" or stray "bar\"" remnants.
    expect(plan.caveats.some((c) => c === 'tool == "foo')).toBe(false);
    expect(plan.caveats.some((c) => c === 'bar"')).toBe(false);
  });

  // Case 2. Attack: a string literal that *is* the word AND.
  // `caller == "AND"` would, under a naive splitter, produce
  // `caller == "` and `"` fragments. Tool predicate is structurally
  // analogous to Case 1 but isolates the bare-AND-as-value variant.
  it("preserves a string literal whose value is exactly \"AND\"", () => {
    const c = makeClassification(
      entry({
        tool: "t",
        recommended_caveat: 'tool == "AND" AND caller == "<your-caller-id>"',
      }),
    );
    const out = planCaveats(c, FULL_BINDINGS);
    const plan = out.plans[0]!;
    // The literal "AND" stays inside its quotes; only the unquoted
    // top-level AND was honoured as a separator.
    expect(plan.caveats).toContain('tool == "AND"');
    expect(plan.caveats).toContain('caller == "agent:planner"');
    expect(plan.caveats).toHaveLength(2);
  });
});

describe("planCaveats — adversarial: empty / whitespace / comment-only caveats", () => {
  // Case 3. Attack: classifier emitted "" (e.g. a buggy upstream rule
  // returned no string). No placeholders means the
  // `unsubstituted_placeholder` flag rule is moot. Defense: function
  // does not crash; caveats array is empty. We document the resulting
  // flag set explicitly so future changes can't silently regress.
  it("does not crash on empty recommended_caveat; emits empty caveats array", () => {
    const c = makeClassification(entry({ recommended_caveat: "" }));
    const out = planCaveats(c, FULL_BINDINGS);
    const plan = out.plans[0]!;
    expect(plan.caveats).toEqual([]);
    // No placeholders → unsubstituted_placeholder must NOT fire.
    expect(plan.flag_reasons).not.toContain("unsubstituted_placeholder");
    // Note: the current contract does not specifically flag an empty
    // caveats array. This pins that contract — if we ever introduce
    // an `empty_caveats` flag reason, this assertion forces an
    // explicit code review.
  });

  // Case 4. Same as Case 3 but with whitespace-only input. The
  // pre-split `.trim()`+`.filter(length > 0)` should drop the empty
  // fragment, yielding the same empty array.
  it("does not crash on whitespace-only recommended_caveat; emits empty caveats array", () => {
    const c = makeClassification(entry({ recommended_caveat: "   \t  " }));
    const out = planCaveats(c, FULL_BINDINGS);
    const plan = out.plans[0]!;
    expect(plan.caveats).toEqual([]);
    expect(plan.flag_reasons).not.toContain("unsubstituted_placeholder");
  });

  // Case 5. Comment captured, no predicates. Defense: comment is
  // surfaced for operator review even when predicates are empty —
  // useful when the classifier wants to leave a note ("review by
  // hand") with no machine-issuable caveat.
  it("captures trailing // comment when there are no predicates", () => {
    const c = makeClassification(
      entry({ recommended_caveat: "  // comment only" }),
    );
    const out = planCaveats(c, FULL_BINDINGS);
    const plan = out.plans[0]!;
    expect(plan.caveats).toEqual([]);
    expect(plan.comment).toBe("comment only");
  });
});

describe("planCaveats — adversarial: unicode / control characters", () => {
  // Case 6. Attack: a malicious server names a tool with a Cyrillic
  // 'а' (U+0430) that visually matches ASCII 'a'. Operators reading
  // the caveats document would think they're constraining `cat` when
  // in fact `cаt` (Cyrillic-a) bypasses string-equality. Defense:
  // we do NOT silently normalise — operators see the exact bytes
  // they will issue against, so they can spot the lookalike during
  // review.
  it("preserves unicode lookalikes verbatim (no normalization)", () => {
    const cyrillicA = "а"; // Cyrillic 'а'
    const tool = `c${cyrillicA}t`;
    const c = makeClassification(
      entry({
        tool,
        recommended_caveat: `tool == "${tool}" AND caller == "<your-caller-id>"`,
      }),
    );
    const out = planCaveats(c, FULL_BINDINGS);
    const plan = out.plans[0]!;
    // The Cyrillic codepoint must round-trip — no NFC/NFKC folding.
    expect(plan.tool).toBe(tool);
    expect(plan.tool.includes(cyrillicA)).toBe(true);
    expect(plan.caveats[0]).toBe(`tool == "${tool}"`);
    // Defense proven: an operator scanning the issuance plan can
    // visually detect the homoglyph because it appears literally.
  });

  // Case 7. Attack: bindings.caller carries control characters
  // (newline, NUL, ESC) — possibly because the operator pasted from
  // a malformed source. Defense: substitute() does not crash; the
  // chars survive verbatim. Documented behavior: capnagent's parser
  // is the boundary that rejects unprintables; we don't pre-sanitize
  // because that would silently mask configuration mistakes.
  it("does not crash when bindings.caller contains control characters", () => {
    const c = makeClassification(entry({}));
    const evilCaller = "agent\n:planner\x00\x1b";
    const out = planCaveats(c, { ...FULL_BINDINGS, caller: evilCaller });
    const plan = out.plans[0]!;
    // Pinned: we pass through, do not sanitize. If we ever decide to
    // sanitize, this test should be updated alongside the doc.
    expect(plan.caveats.some((c) => c.includes(evilCaller))).toBe(true);
  });
});

describe("planCaveats — adversarial: oversized inputs", () => {
  // Case 8. Attack: classifier produced (or was tricked into
  // producing) a 100KB recommended_caveat. Defense: planCaveats runs
  // in roughly linear time on input size — no quadratic regex
  // backtracking, no OOM. We assert "reasonable runtime" by setting
  // a generous wall-clock budget that still catches catastrophic
  // backtracking (which would blow past it by orders of magnitude).
  it("parses a 100KB AND-joined caveat string without OOM or timeout", () => {
    // ~100KB of valid `arg.x_<i> == "<i>"` fragments AND-joined.
    const fragments: string[] = [];
    let total = 0;
    let i = 0;
    while (total < 100_000) {
      const f = `arg.k${i} == "v${i}"`;
      fragments.push(f);
      total += f.length + 5; // +5 for " AND "
      i++;
    }
    const big = fragments.join(" AND ");
    expect(big.length).toBeGreaterThanOrEqual(100_000);
    const c = makeClassification(
      entry({ recommended_caveat: big, confused_deputy_candidate: false }),
    );
    const start = Date.now();
    const out = planCaveats(c, FULL_BINDINGS);
    const elapsed = Date.now() - start;
    // Generous budget — current implementation completes in <100ms.
    // Catastrophic backtracking would exceed seconds.
    expect(elapsed).toBeLessThan(2_000);
    expect(out.plans[0]!.caveats.length).toBe(fragments.length);
  });
});

describe("planCaveats — adversarial: per_tool_overrides edge cases", () => {
  // Case 9. Attack: an operator (or a tool that synthesises overrides)
  // drops a string with AND keywords into per_tool_overrides. Per the
  // contract, overrides are passthrough — they are NOT re-split on
  // AND, because they are already in capnagent DSL form. Defense:
  // a multi-clause override string lands in caveats[] verbatim.
  it("does not re-split AND keywords inside per_tool_overrides values", () => {
    const c = makeClassification(entry({ tool: "foo" }));
    const overrideValue =
      'arg.x == "alpha AND beta" AND arg.y > 0';
    const out = planCaveats(c, {
      ...FULL_BINDINGS,
      per_tool_overrides: { foo: [overrideValue] },
    });
    const plan = out.plans[0]!;
    // Defense proven: the override appears as ONE entry, not split.
    expect(plan.caveats).toContain(overrideValue);
    expect(plan.caveats.filter((s) => s === overrideValue)).toHaveLength(1);
  });

  // Case 12. Attack: an operator typoes a tool name in
  // per_tool_overrides ("foo" instead of "fooBar"), and `foo` does
  // not exist in the classification. Defense: the override is
  // silently dropped. The critical property is that it is NOT
  // applied to a *different* tool (e.g. by index or fuzzy match) —
  // that would be the security-relevant misbehavior.
  it("silently drops per_tool_overrides for tools that don't exist", () => {
    const c = makeClassification(entry({ tool: "real_tool" }));
    const out = planCaveats(c, {
      ...FULL_BINDINGS,
      per_tool_overrides: {
        nonexistent_tool: ['arg.evil == "hijack"'],
      },
    });
    const plan = out.plans[0]!;
    // Defense proven: real_tool's caveats do NOT contain the
    // override that was keyed under a different name.
    expect(plan.caveats.every((c) => !c.includes('arg.evil'))).toBe(true);
    expect(plan.caveats.every((c) => !c.includes('hijack'))).toBe(true);
    // And the plan summary is unchanged in shape.
    expect(out.summary.total).toBe(1);
  });
});

describe("planCaveats — adversarial: confidence out of range", () => {
  // Case 10. Attack: a classifier bug or a malicious classifier
  // produces confidence outside [0, 1] (1.5 or -0.1). Defense: the
  // function does not crash; the existing flag rule (< 0.5 →
  // low_confidence) behaves consistently — 1.5 means NOT
  // low_confidence, -0.1 IS low_confidence. We do not silently clamp,
  // because clamping would hide the upstream bug.
  it("does not crash on confidence > 1 and treats it as not-low-confidence", () => {
    const c = makeClassification(entry({ confidence: 1.5 }));
    const out = planCaveats(c, FULL_BINDINGS);
    expect(out.plans[0]!.flag_reasons).not.toContain("low_confidence");
  });

  it("does not crash on negative confidence and flags it as low_confidence", () => {
    const c = makeClassification(entry({ confidence: -0.1 }));
    const out = planCaveats(c, FULL_BINDINGS);
    expect(out.plans[0]!.flag_reasons).toContain("low_confidence");
  });
});

describe("planCaveats — adversarial: pinned 'double-quote bug' regression", () => {
  // Case 11. Pin the historical fix: when the operator binds caller
  // to a value that ALREADY contains surrounding double-quotes
  // ("agent:planner"), the substituted output ends up with doubled
  // quotes (caller == ""agent:planner""). The fix is documented in
  // substitute()'s docstring: mcp-recon's recommended_caveat already
  // wraps placeholders in quotes, so JSON.stringify-ing the binding
  // would double-quote. Pinning that we DO get doubled quotes when
  // the operator pre-quotes ensures the substitution stays naive
  // string replacement (correct) and does not silently strip the
  // operator's quotes (which would mask the operator's mistake and
  // potentially produce a syntactically invalid caveat).
  it("pins the substitute() contract: pre-quoted bindings produce doubled quotes (operator must not pre-quote)", () => {
    const c = makeClassification(entry({}));
    // Operator passes a pre-quoted value — a known-mistake input.
    const out = planCaveats(c, { ...FULL_BINDINGS, caller: '"agent:planner"' });
    const callerCaveat = out.plans[0]!.caveats.find((c) =>
      c.startsWith("caller =="),
    );
    expect(callerCaveat).toBeDefined();
    // The caveat segment is exactly: caller == ""agent:planner""
    // i.e. two pairs of doubled-up quotes (4 quote chars total in
    // this segment; 2 occurrences of the doubled-quote substring).
    expect(callerCaveat).toBe('caller == ""agent:planner""');
    const matches = callerCaveat!.match(/""/g) ?? [];
    expect(matches).toHaveLength(2);
    // Why this matters: the symmetric well-formed test in
    // caveats.test.ts checks that an *unquoted* binding produces NO
    // `""` substrings. Together they pin that substitution is naive
    // and that pre-quoting is the operator's mistake to surface, not
    // ours to silently fix.
  });
});
