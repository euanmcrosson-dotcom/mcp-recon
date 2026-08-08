/**
 * Encoding-tricks axis.
 *
 * For each string-typed arg, send variants that round-trip through
 * common middleware in surprising ways: percent-encoded null bytes,
 * Unicode homographs, RTL overrides, double-encoded sequences.
 *
 * Per docs/SPEC.md §"Fuzzing strategy".
 */

import type { Prng } from "../prng.js";
import type { ToolFacts } from "../schema.js";

const ENCODING_PAYLOADS: Array<{ value: string; reason: string }> = [
	{ value: "%00", reason: "percent-encoded NUL" },
	{ value: "%2e%2e", reason: "percent-encoded dot-dot" },
	{ value: "%252e%252e", reason: "double-percent-encoded dot-dot" },
	{ value: "x\x00y", reason: "embedded NUL byte" },
	// Cyrillic homograph for ASCII 'a' (U+0430)
	{ value: "аdmin", reason: "Cyrillic-a homograph (admin)" },
	// RTL override
	{ value: "‮gnp.exe", reason: "RTL override (filename spoof)" },
	// Backslash escapes
	{ value: "\\..\\..\\..", reason: "backslash traversal" },
	// Mixed slashes (Windows-style)
	{ value: "x\\y/z", reason: "mixed-separator path" },
	// Unicode normalization edge — combining diacritics
	{ value: "café", reason: "combining diacritic (NFC vs NFD)" },
	// Long string with one funny byte
	{ value: `${"a".repeat(1024)}\x00`, reason: "long string with trailing NUL" },
];

/** Generate encoding-trick inputs for one tool. */
export function* generateEncoding(
	facts: ToolFacts,
	_prng: Prng,
): Generator<{ args: Record<string, unknown>; rationale: string }> {
	if (facts.isArgless) return;

	for (const arg of facts.args) {
		const argName = arg.path[0];
		if (argName === undefined) continue;
		if (arg.declaredType !== "string" && arg.declaredType !== "unknown")
			continue;

		const baseArgs = baseShape(facts);
		for (const { value, reason } of ENCODING_PAYLOADS) {
			yield {
				args: { ...baseArgs, [argName]: value },
				rationale: `encoding "${argName}": ${reason}`,
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
		out[argName] = arg.declaredType === "string" ? "x" : 0;
	}
	return out;
}
