/**
 * Type-confusion axis.
 *
 * For each typed arg, send a value of the wrong type. Detects servers
 * that don't validate types and pass through to weakly-typed runtimes
 * (e.g. JS runtime that coerces silently).
 *
 * Per docs/SPEC.md §"Fuzzing strategy".
 */

import type { Prng } from "../prng.js";
import type { SchemaType, ToolFacts } from "../schema.js";

const ALL_TYPES: SchemaType[] = [
  "string",
  "number",
  "integer",
  "boolean",
  "array",
  "object",
  "null",
];

/** A representative value of each primitive type for substitution. */
const SAMPLE_OF: Record<SchemaType, unknown> = {
  string: "wrong-type-here",
  number: 3.14,
  integer: 42,
  boolean: true,
  array: ["element"],
  object: { nested: true },
  null: null,
};

/** Generate type-confused inputs for one tool. */
export function* generateTypeConfusion(
  facts: ToolFacts,
  _prng: Prng,
): Generator<{ args: Record<string, unknown>; rationale: string }> {
  if (facts.isArgless) return;

  for (const arg of facts.args) {
    const argName = arg.path[0];
    if (argName === undefined) continue;
    if (arg.declaredType === "unknown") continue;

    // Substitute a value of every other type.
    for (const otherType of ALL_TYPES) {
      if (otherType === arg.declaredType) continue;
      const baseArgs = baseShape(facts);
      yield {
        args: { ...baseArgs, [argName]: SAMPLE_OF[otherType] },
        rationale: `type-confusion arg "${argName}" declared ${arg.declaredType}, sent ${otherType}`,
      };
    }
  }
}

function baseShape(facts: ToolFacts): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const arg of facts.args) {
    const argName = arg.path[0];
    if (argName === undefined) continue;
    if (!arg.required) continue;
    if (arg.declaredType === "unknown") {
      out[argName] = "x";
      continue;
    }
    out[argName] = SAMPLE_OF[arg.declaredType];
  }
  return out;
}
