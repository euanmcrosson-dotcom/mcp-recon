/**
 * Boundary-values axis.
 *
 * For each declared arg, generate edge values that often expose
 * uninitialised handling: empty string, max-length string, 0, -1,
 * Number.MAX_SAFE_INTEGER + 1, NaN, null, undefined-as-omitted.
 *
 * Per docs/SPEC.md §"Fuzzing strategy".
 */

import type { Prng } from "../prng.js";
import type { ToolFacts } from "../schema.js";

const STRING_BOUNDARIES = [
	"",
	"x".repeat(1024 * 64), // 64 KiB
	"\x00",
	"\x00\x00\x00",
	" ",
	"\t\n\r",
];

const NUMBER_BOUNDARIES = [
	0,
	-1,
	-0.0,
	Number.MAX_SAFE_INTEGER + 1,
	-Number.MAX_SAFE_INTEGER - 1,
	Number.EPSILON,
	Number.NaN,
	Number.POSITIVE_INFINITY,
	Number.NEGATIVE_INFINITY,
];

const INTEGER_BOUNDARIES = [
	0,
	-1,
	Number.MAX_SAFE_INTEGER,
	Number.MAX_SAFE_INTEGER + 1,
	-Number.MAX_SAFE_INTEGER,
];

/** Generate boundary inputs for one tool. */
export function* generateBoundary(
	facts: ToolFacts,
	_prng: Prng,
): Generator<{ args: Record<string, unknown>; rationale: string }> {
	if (facts.isArgless) {
		yield { args: {}, rationale: "argless tool: empty args object" };
		return;
	}

	for (const arg of facts.args) {
		const argName = arg.path[0];
		if (argName === undefined) continue;

		const baseArgs = baseShape(facts);
		const candidates = boundaryValuesFor(arg.declaredType);
		for (const value of candidates) {
			yield {
				args: { ...baseArgs, [argName]: value },
				rationale: `boundary ${arg.declaredType} for arg "${argName}": ${describeValue(value)}`,
			};
		}
	}

	// Whole-arg-missing: required field omitted.
	for (const arg of facts.args) {
		const argName = arg.path[0];
		if (argName === undefined) continue;
		if (!arg.required) continue;
		const baseArgs = baseShape(facts);
		delete baseArgs[argName];
		yield {
			args: baseArgs,
			rationale: `boundary: omit required field "${argName}"`,
		};
	}
}

function boundaryValuesFor(
	t: ToolFacts["args"][number]["declaredType"],
): readonly unknown[] {
	switch (t) {
		case "string":
			return [...STRING_BOUNDARIES, null];
		case "number":
			return [...NUMBER_BOUNDARIES, null];
		case "integer":
			return [...INTEGER_BOUNDARIES, null];
		case "boolean":
			return [true, false, null];
		case "array":
			return [[], [null], Array.from({ length: 1024 }, (_, i) => i)];
		case "object":
			return [{}, { __proto__: null }, { a: { b: { c: { d: { e: 1 } } } } }];
		case "null":
			return [null];
		case "unknown":
			// For unknown-type args, throw the kitchen sink at it.
			return ["", 0, true, [], {}, null];
	}
}

function describeValue(v: unknown): string {
	if (v === null) return "null";
	if (Number.isNaN(v as number)) return "NaN";
	if (v === Number.POSITIVE_INFINITY) return "+Infinity";
	if (v === Number.NEGATIVE_INFINITY) return "-Infinity";
	if (typeof v === "string") {
		if (v.length === 0) return "empty string";
		if (v.length > 32) return `string len=${v.length}`;
		return JSON.stringify(v);
	}
	if (Array.isArray(v)) return `array len=${v.length}`;
	return typeof v;
}

/** Build a "default" args shape so a single-field fuzz still has the other required fields populated. */
function baseShape(facts: ToolFacts): Record<string, unknown> {
	const out: Record<string, unknown> = {};
	for (const arg of facts.args) {
		const argName = arg.path[0];
		if (argName === undefined) continue;
		if (!arg.required) continue;
		out[argName] = defaultForType(arg.declaredType);
	}
	return out;
}

function defaultForType(t: ToolFacts["args"][number]["declaredType"]): unknown {
	switch (t) {
		case "string":
			return "x";
		case "number":
		case "integer":
			return 1;
		case "boolean":
			return false;
		case "array":
			return [];
		case "object":
			return {};
		case "null":
			return null;
		case "unknown":
			return "x";
	}
}
