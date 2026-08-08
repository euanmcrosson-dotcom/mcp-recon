/**
 * `classify` — turn an inventory (+ optional fuzz results) into a
 * v0.1 classification document.
 *
 * Methodology in docs/METHODOLOGY.md. The rules live in `./rules.ts`.
 * Confidence combines via noisy-OR; authority is the lattice-join of
 * rule floors plus side-effect-verb escalations.
 */

import type { EnumeratedTool, ToolInventory } from "../enumerate.js";
import { type ToolFacts, extractToolFacts } from "../fuzz/schema.js";
import type { FuzzResults } from "../fuzz/types.js";
import { synthesizeCaveat } from "./caveat.js";
import {
	DESCRIPTION_MATCH_WEIGHT,
	DESTRUCTIVE_VERBS,
	FUZZ_INFORMED_BONUS,
	NAME_MATCH_WEIGHT,
	type NameOrDescRule,
	PRIVILEGED_VERBS,
	RULES,
	SCHEMA_MATCH_WEIGHT,
	SIDE_EFFECT_VERBS,
	escalateAuthority,
	maxAuthority,
} from "./rules.js";
import {
	type AuthorityLevel,
	CLASSIFICATION_SCHEMA,
	type Classification,
	type ClassificationResults,
	type DataClass,
} from "./types.js";

export { CLASSIFICATION_SCHEMA } from "./types.js";
export type {
	AuthorityLevel,
	Classification,
	ClassificationResults,
	DataClass,
} from "./types.js";

/** Classify every tool in `inventory`. Optionally fold in `fuzz` for confidence. */
export function classify(
	inventory: ToolInventory,
	fuzz?: FuzzResults,
): ClassificationResults {
	const fuzzByTool = new Map<string, { ok: number; total: number }>();
	if (fuzz) {
		for (const s of fuzz.summary) {
			fuzzByTool.set(s.tool, {
				ok: s.ok,
				total: s.ok + s.protocol_error + s.runtime_error,
			});
		}
	}

	const classifications = inventory.tools.map((tool) =>
		classifyTool(tool, fuzzByTool.get(tool.name)),
	);

	return {
		schema: CLASSIFICATION_SCHEMA,
		scanned_at: new Date().toISOString(),
		server: inventory.server,
		fuzz_informed: fuzz !== undefined,
		classifications,
	};
}

function classifyTool(
	tool: EnumeratedTool,
	fuzzStats?: { ok: number; total: number },
): Classification {
	const facts = extractToolFacts(tool.name, tool.inputSchema);
	const description = tool.description ?? "";
	const name = tool.name;

	// Per-fired-rule confidences and the (data_class, authority_floor)
	// they assert. Final assignment = noisy-OR over the highest-weighted
	// rule per data-class, authority = lattice-join of all floors plus
	// side-effect-verb escalations.
	type Fired = {
		rule: NameOrDescRule;
		weight: number;
		where: "name" | "description";
	};
	const fired: Fired[] = [];

	for (const rule of RULES) {
		if (rule.scope !== "description" && rule.pattern.test(name)) {
			fired.push({ rule, weight: NAME_MATCH_WEIGHT, where: "name" });
		}
		if (rule.scope !== "name" && rule.pattern.test(description)) {
			fired.push({
				rule,
				weight: DESCRIPTION_MATCH_WEIGHT,
				where: "description",
			});
		}
	}

	// Schema-shape signal — adds to filesystem / network if arg names
	// are path-shaped or URL-shaped.
	const schemaFired: Array<{
		data_class: DataClass;
		weight: number;
		reason: string;
	}> = [];
	for (const arg of facts.args) {
		if (arg.isPathShaped) {
			schemaFired.push({
				data_class: "filesystem",
				weight: SCHEMA_MATCH_WEIGHT,
				reason: `arg "${arg.path[0]}" is path-shaped`,
			});
		}
		if (arg.isUrlShaped) {
			schemaFired.push({
				data_class: "network",
				weight: SCHEMA_MATCH_WEIGHT,
				reason: `arg "${arg.path[0]}" is URL-shaped`,
			});
		}
		if (arg.isCommandShaped) {
			schemaFired.push({
				data_class: "shell",
				weight: SCHEMA_MATCH_WEIGHT,
				reason: `arg "${arg.path[0]}" is command-shaped`,
			});
		}
	}

	// Bucket evidence by data_class. Per-bucket noisy-OR over rule
	// confidences gives us the final per-class confidence; the winner
	// is the highest.
	const evidenceByClass = new Map<DataClass, number[]>();
	const floorsByClass = new Map<DataClass, AuthorityLevel>();
	const rationaleParts: string[] = [];

	for (const f of fired) {
		pushEvidence(evidenceByClass, f.rule.data_class, f.weight);
		floorsByClass.set(
			f.rule.data_class,
			maxAuthority(
				floorsByClass.get(f.rule.data_class) ?? f.rule.authority_floor,
				f.rule.authority_floor,
			),
		);
		rationaleParts.push(
			`${f.where} match "${f.rule.pattern.source}" → ${f.rule.data_class}/${f.rule.authority_floor} (${f.weight.toFixed(2)})`,
		);
	}

	for (const s of schemaFired) {
		pushEvidence(evidenceByClass, s.data_class, s.weight);
		rationaleParts.push(
			`schema: ${s.reason} → ${s.data_class} (${s.weight.toFixed(2)})`,
		);
	}

	// Pick the highest-confidence data-class.
	let winnerClass: DataClass = "unknown";
	let winnerConfidence = 0;
	for (const [dc, confidences] of evidenceByClass.entries()) {
		const combined = noisyOr(confidences);
		if (combined > winnerConfidence) {
			winnerClass = dc;
			winnerConfidence = combined;
		}
	}

	// Apply fuzz-informed bonus: if the server actually accepted any of
	// our adversarial inputs, raise confidence we know what kind of
	// tool this is (the fuzzer crossed enough surface to confirm a
	// real interaction, even if accepted inputs are not "successes").
	if (fuzzStats && fuzzStats.total > 0 && fuzzStats.ok > 0) {
		winnerConfidence = noisyOr([winnerConfidence, FUZZ_INFORMED_BONUS]);
		rationaleParts.push(
			`fuzz: ${fuzzStats.ok}/${fuzzStats.total} accepted → +${FUZZ_INFORMED_BONUS}`,
		);
	}

	// Authority: the floor for the winning class, escalated by any
	// side-effect verbs in the description.
	let authority: AuthorityLevel = floorsByClass.get(winnerClass) ?? "read";
	for (const verb of PRIVILEGED_VERBS) {
		if (verb.test(description)) {
			authority = "privileged";
			rationaleParts.push(
				"description has privileged verb → escalate to privileged",
			);
		}
	}
	for (const verb of DESTRUCTIVE_VERBS) {
		if (verb.test(description)) {
			authority = maxAuthority(authority, "destructive");
			rationaleParts.push(
				"description has destructive verb → escalate to destructive",
			);
		}
	}
	// Generic side-effect verbs only escalate by ONE step, and only if
	// we haven't already saturated.
	for (const verb of SIDE_EFFECT_VERBS) {
		if (verb.test(description) && authority === "read") {
			authority = escalateAuthority(authority);
			rationaleParts.push(
				"description has side-effect verb → escalate one step",
			);
			break;
		}
	}

	// Confused-deputy flag: tool takes user-controllable string args
	// AND has write/destructive/privileged authority. (METHODOLOGY.md
	// §"The confused-deputy flag")
	const hasUserStringArg = facts.args.some(
		(a) =>
			(a.declaredType === "string" || a.declaredType === "unknown") &&
			a.enumValues === undefined,
	);
	const writeOrAbove = authority !== "read";
	const confusedDeputy = hasUserStringArg && writeOrAbove;

	if (confusedDeputy) {
		rationaleParts.push(
			"user-controllable string arg + non-read authority → confused-deputy candidate",
		);
	}

	const recommended_caveat = synthesizeCaveat({
		tool: name,
		data_class: winnerClass,
		authority_level: authority,
		facts,
	});

	return {
		tool: name,
		data_class: winnerClass,
		authority_level: authority,
		confused_deputy_candidate: confusedDeputy,
		confidence: round(winnerConfidence, 3),
		rationale:
			rationaleParts.length === 0
				? "no rules fired"
				: rationaleParts.join("; "),
		recommended_caveat,
	};
}

function pushEvidence(
	map: Map<DataClass, number[]>,
	dc: DataClass,
	w: number,
): void {
	const arr = map.get(dc);
	if (arr) arr.push(w);
	else map.set(dc, [w]);
}

/** Noisy-OR combiner: 1 - ∏(1 - c_i). */
export function noisyOr(confidences: readonly number[]): number {
	if (confidences.length === 0) return 0;
	let product = 1;
	for (const c of confidences) {
		product *= 1 - clamp(c, 0, 1);
	}
	return clamp(1 - product, 0, 1);
}

function clamp(x: number, lo: number, hi: number): number {
	if (x < lo) return lo;
	if (x > hi) return hi;
	return x;
}

function round(x: number, decimals: number): number {
	const f = 10 ** decimals;
	return Math.round(x * f) / f;
}

// Re-export the synthesizer / facts for downstream tools that want it.
export { synthesizeCaveat } from "./caveat.js";
export type { ToolFacts };
