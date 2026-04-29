/**
 * Schema-violation axis.
 *
 * Send args that violate the declared schema in structural ways:
 * extra fields the schema doesn't mention, missing required fields,
 * wrong types in nested positions, deeply-nested objects,
 * prototype pollution shapes.
 *
 * Per docs/SPEC.md §"Fuzzing strategy".
 */

import type { Prng } from "../prng.js";
import type { ToolFacts } from "../schema.js";

/** Generate schema-violation inputs for one tool. */
export function* generateSchemaViolation(
  facts: ToolFacts,
  _prng: Prng,
): Generator<{ args: Record<string, unknown>; rationale: string }> {
  // Even for argless tools, send unexpected payloads.
  yield {
    args: { __injected: "x" } as Record<string, unknown>,
    rationale: "schema-violation: extra field on argless tool",
  };
  yield {
    args: { __proto__: { polluted: true } } as Record<string, unknown>,
    rationale: "schema-violation: __proto__ pollution attempt",
  };
  yield {
    args: deepNest(50),
    rationale: "schema-violation: 50-deep nested object",
  };

  if (facts.isArgless) return;

  // Missing-each-required-field — covered by boundary axis too, but
  // the rationale string is different so we keep both for forensic
  // clarity in the fuzz output.
  for (const arg of facts.args) {
    if (!arg.required) continue;
    const argName = arg.path[0];
    if (argName === undefined) continue;
    const stripped = baseShape(facts);
    delete stripped[argName];
    yield {
      args: stripped,
      rationale: `schema-violation: required "${argName}" omitted`,
    };
  }

  // Extra fields alongside valid args.
  const baseArgs = baseShape(facts);
  yield {
    args: { ...baseArgs, __injected: "x" },
    rationale: "schema-violation: extra field __injected",
  };
  yield {
    args: { ...baseArgs, constructor: { prototype: { polluted: true } } },
    rationale: "schema-violation: constructor.prototype pollution attempt",
  };
}

function baseShape(facts: ToolFacts): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const arg of facts.args) {
    const argName = arg.path[0];
    if (argName === undefined) continue;
    if (!arg.required) continue;
    out[argName] = arg.declaredType === "string" ? "x" : 0;
  }
  return out;
}

function deepNest(depth: number): Record<string, unknown> {
  let cur: unknown = "leaf";
  for (let i = 0; i < depth; i++) {
    cur = { nested: cur };
  }
  return { deep: cur };
}
