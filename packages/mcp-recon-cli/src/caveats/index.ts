/**
 * `caveats` — turn a classification into a v0.1 caveats document.
 *
 * Each tool's `recommended_caveat` from the classifier is an
 * AND-joined natural-language predicate string with placeholder
 * tokens (`<your-caller-id>`, `<your-sandbox-prefix>`,
 * `<your-cap-expiry>`). This module:
 *
 *   1. Splits each recommended_caveat on `AND` into individual
 *      capnagent DSL predicates (one per `Issuer.caveat(...)` call).
 *   2. Strips trailing `// comment` and preserves it on the plan.
 *   3. Substitutes operator-supplied bindings into the placeholders.
 *      Missing bindings leave placeholders literal AND flag the plan.
 *   4. Computes a `flagged` decision per plan with structured reasons.
 *   5. Appends per-tool overrides if supplied.
 *
 * The output JSON is the **importable artifact** that bridges
 * mcp-recon → capnagent without requiring operators to copy-paste
 * caveat strings by hand. capnagent's issuer can iterate over each
 * plan's `caveats[]` array directly:
 *
 *     for (const c of plan.caveats) builder = builder.caveat(c);
 *
 * Methodology notes:
 *
 *   - Flag rules are deliberately narrow. Over-flagging trains
 *     operators to ignore flags. The four reasons in `FlagReason`
 *     cover the structural gaps that reliably indicate a config
 *     error; everything softer is left to operator review.
 *   - Bindings are optional by design. Running `caveats` with no
 *     bindings produces a "review pass" — every plan is flagged,
 *     but the operator can scan the output and see exactly which
 *     tools need which bindings before committing values.
 */

import type { ClassificationResults } from "../classify/types.js";
import {
	CAVEATS_SCHEMA,
	type CaveatBindings,
	type CaveatPlan,
	type CaveatsResults,
	type FlagReason,
} from "./types.js";

/** Confidence threshold below which classifications are flagged. */
const LOW_CONFIDENCE_THRESHOLD = 0.5;

const COMMENT_SPLIT = /\s*\/\/\s*/;
const PLACEHOLDER_REMAINING = /<your-[a-z-]+>/i;
const ARG_CONSTRAINT = /\barg\./i;

const PLACEHOLDER_PATTERNS = {
	caller: /<your-caller-id>/g,
	sandbox: /<your-sandbox-prefix>/g,
	expiry: /<your-cap-expiry>/g,
} as const;

/**
 * Build a v0.1 caveats document from a classification.
 *
 * @param classification - the document emitted by `mcp-recon classify`
 * @param bindings       - operator-supplied placeholder values + per-tool overrides
 */
export function planCaveats(
	classification: ClassificationResults,
	bindings: CaveatBindings,
): CaveatsResults {
	const plans = classification.classifications.map((entry) =>
		planOne(entry, bindings),
	);

	const ready = plans.filter((p) => !p.flagged).length;
	const flagged = plans.length - ready;

	return {
		schema: CAVEATS_SCHEMA,
		scanned_at: new Date().toISOString(),
		server: classification.server,
		bindings,
		plans,
		summary: {
			total: plans.length,
			ready,
			flagged,
		},
	};
}

function planOne(
	entry: ClassificationResults["classifications"][number],
	bindings: CaveatBindings,
): CaveatPlan {
	const { caveats: rawCaveats, comment } = parseRecommendedCaveat(
		entry.recommended_caveat,
	);
	const substituted = rawCaveats.map((c) => substitute(c, bindings));

	const overrides = bindings.per_tool_overrides?.[entry.tool] ?? [];
	const all_caveats = [...substituted, ...overrides];

	const flag_reasons: FlagReason[] = [];
	if (entry.data_class === "unknown") {
		flag_reasons.push("classification_unknown");
	}
	if (entry.confidence < LOW_CONFIDENCE_THRESHOLD) {
		flag_reasons.push("low_confidence");
	}
	if (entry.confused_deputy_candidate && !hasArgConstraint(all_caveats)) {
		flag_reasons.push("cdc_without_arg_constraint");
	}
	if (all_caveats.some((c) => PLACEHOLDER_REMAINING.test(c))) {
		flag_reasons.push("unsubstituted_placeholder");
	}

	const purpose = purposeFromEntry(entry);

	return {
		tool: entry.tool,
		data_class: entry.data_class,
		authority_level: entry.authority_level,
		confused_deputy_candidate: entry.confused_deputy_candidate,
		purpose,
		caveats: all_caveats,
		flagged: flag_reasons.length > 0,
		flag_reasons,
		...(comment !== undefined ? { comment } : {}),
	};
}

function parseRecommendedCaveat(raw: string): {
	caveats: string[];
	comment?: string;
} {
	const parts = raw.split(COMMENT_SPLIT);
	const predicates_raw = parts[0] ?? "";
	const comment = parts[1]?.trim();
	const caveats = splitOnUnquotedAnd(predicates_raw)
		.map((s) => s.trim())
		.filter((s) => s.length > 0);
	return comment ? { caveats, comment } : { caveats };
}

/**
 * Split on ` AND ` only when not inside a double-quoted string literal.
 *
 * The naive `string.split(/\s+AND\s+/i)` mis-splits when a tool name
 * (or other string-literal value) contains the word `AND` — e.g.
 *
 *     tool == "foo AND bar" AND caller == "x"
 *
 * would split into three fragments and break the tool predicate.
 * Tool names propagate from the upstream MCP server, so a malicious
 * server can synthesise this. This walker tracks an `inQuotes` flag
 * and only treats ` AND ` as a separator when outside quotes.
 *
 * Pinned by `caveats.adversarial.test.ts` — cases 1 and 2.
 */
function splitOnUnquotedAnd(input: string): string[] {
	const out: string[] = [];
	let buf = "";
	let inQuotes = false;
	let i = 0;
	while (i < input.length) {
		const ch = input[i];
		if (ch === '"') {
			inQuotes = !inQuotes;
			buf += ch;
			i++;
			continue;
		}
		if (!inQuotes && /\s/.test(ch ?? "")) {
			const m = input.slice(i).match(/^\s+AND\s+/i);
			if (m) {
				out.push(buf);
				buf = "";
				i += m[0].length;
				continue;
			}
		}
		buf += ch;
		i++;
	}
	out.push(buf);
	return out;
}

/**
 * Substitute placeholders. Important: mcp-recon's recommended_caveat
 * already wraps placeholder tokens in surrounding quotes where they
 * need to be string-literal-shaped:
 *
 *     caller == "<your-caller-id>"
 *     arg.path starts_with "<your-sandbox-prefix>/"
 *     now <= @<your-cap-expiry>
 *
 * So we substitute the RAW value; the surrounding quotes from the
 * source preserve the string-literal shape. Adding JSON.stringify()
 * here would double-quote the result.
 */
function substitute(caveat: string, bindings: CaveatBindings): string {
	let out = caveat;
	if (bindings.caller !== undefined) {
		out = out.replaceAll(PLACEHOLDER_PATTERNS.caller, bindings.caller);
	}
	if (bindings.sandbox_prefix !== undefined) {
		out = out.replaceAll(PLACEHOLDER_PATTERNS.sandbox, bindings.sandbox_prefix);
	}
	if (bindings.expiry !== undefined) {
		out = out.replaceAll(PLACEHOLDER_PATTERNS.expiry, bindings.expiry);
	}
	return out;
}

function hasArgConstraint(caveats: readonly string[]): boolean {
	return caveats.some((c) => ARG_CONSTRAINT.test(c));
}

function purposeFromEntry(
	entry: ClassificationResults["classifications"][number],
): string {
	const tag = entry.confused_deputy_candidate ? "cdc" : entry.authority_level;
	return `${entry.data_class}.${tag}.${entry.tool}`;
}

export { CAVEATS_SCHEMA } from "./types.js";
export type {
	CaveatBindings,
	CaveatPlan,
	CaveatsResults,
	FlagReason,
} from "./types.js";
