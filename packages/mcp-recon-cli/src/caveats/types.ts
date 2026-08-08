/**
 * Wire-format types for the v0.1 caveats document.
 *
 * Schema tag: `mcp-recon/v0.1/caveats`. The caveats document is the
 * **importable artifact** that closes the bridge to capnagent: a
 * structured JSON document of capnagent-ready caveat plans, with
 * placeholder substitution applied (or flagged for review when
 * substitutions are missing).
 *
 * This is downstream of `mcp-recon/v0.1/classification` — every plan
 * traces back to one classification entry. The `recommended_caveat`
 * string in classifications is human-readable; the plans here are
 * machine-readable and feed into a capnagent issuer directly.
 */

import type { AuthorityLevel, DataClass } from "../classify/types.js";

/** Schema-version tag for caveats documents. */
export const CAVEATS_SCHEMA = "mcp-recon/v0.1/caveats" as const;

/**
 * Operator-supplied bindings for placeholder tokens in the
 * classifier's `recommended_caveat` strings. Any binding may be
 * omitted; omitted bindings leave their placeholders unsubstituted
 * AND flag the plan for review. This lets operators run
 * `mcp-recon caveats classification.json` without bindings to see
 * exactly what they need to bind before issuance.
 */
export interface CaveatBindings {
	/** Substitutes `<your-caller-id>`. */
	caller?: string;
	/** Substitutes `<your-sandbox-prefix>`. */
	sandbox_prefix?: string;
	/** Substitutes `<your-cap-expiry>`. ISO-8601 string. */
	expiry?: string;
	/**
	 * Optional per-tool caveat overrides. Each entry's caveats are
	 * appended after the substituted ones — useful for tightening
	 * confused-deputy candidates the classifier didn't constrain.
	 */
	per_tool_overrides?: Record<string, string[]>;
}

/** Why a plan was flagged (zero or more reasons). */
export type FlagReason =
	/** Classifier returned `unknown` for this tool — operator must classify by hand. */
	| "classification_unknown"
	/** Classifier confidence < 0.5 — review before trusting. */
	| "low_confidence"
	/** Tool is a confused-deputy candidate but no `arg.*` constraint after substitution. */
	| "cdc_without_arg_constraint"
	/** A `<your-...>` placeholder remains in at least one caveat. */
	| "unsubstituted_placeholder";

/** One issuance plan per classified tool. */
export interface CaveatPlan {
	/** Tool name from the classification. */
	tool: string;
	/** Pass-through from classification. */
	data_class: DataClass;
	/** Pass-through from classification. */
	authority_level: AuthorityLevel;
	/** Pass-through from classification. */
	confused_deputy_candidate: boolean;
	/** Operator-readable purpose string for the issuer. */
	purpose: string;
	/** Caveats to apply to the issuer, one DSL predicate per array entry. */
	caveats: string[];
	/** True if the plan needs review before issuance. */
	flagged: boolean;
	/** Specific flag reasons, when flagged. */
	flag_reasons: FlagReason[];
	/**
	 * Free-form trailing comment from the classifier's
	 * `recommended_caveat` (e.g. "READ filesystem; bound the sandbox
	 * prefix tightly"). Preserved for operator review.
	 */
	comment?: string;
}

/** Top-level caveats document. */
export interface CaveatsResults {
	schema: typeof CAVEATS_SCHEMA;
	scanned_at: string;
	server: { name?: string; version?: string };
	bindings: CaveatBindings;
	plans: CaveatPlan[];
	summary: {
		/** Total plans (one per classified tool). */
		total: number;
		/** Plans with `flagged === false` — directly issuable. */
		ready: number;
		/** Plans with `flagged === true` — review required. */
		flagged: number;
	};
}
