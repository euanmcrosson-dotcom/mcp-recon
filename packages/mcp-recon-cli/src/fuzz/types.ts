/**
 * Wire-format types for the v0.1 fuzz document. Mirrors the Rust core's
 * `mcp_recon_core::fuzzer` types so the two stay in lock-step.
 *
 * Schema tag: `mcp-recon/v0.1/fuzz`. Downstream consumers (classifier,
 * reporter, capnagent's caveat-suggestion bridge) parse based on this
 * string.
 */

/** Schema-version tag for fuzz documents. */
export const FUZZ_SCHEMA = "mcp-recon/v0.1/fuzz" as const;

/** Six categorical axes (per docs/SPEC.md §"Fuzzing strategy"). */
export type FuzzAxis =
	| "boundary_values"
	| "type_confusion"
	| "encoding_tricks"
	| "path_traversal"
	| "url_hostility"
	| "schema_violation";

/** What the underlying tool / transport did with the fuzz input. */
export type FuzzOutcome =
	| { kind: "ok"; snippet: string }
	| { kind: "protocol_error"; message: string }
	| { kind: "runtime_error"; message: string };

/** One recorded fuzz interaction. */
export interface FuzzCall {
	tool: string;
	axis: FuzzAxis;
	rationale: string;
	arguments: Record<string, unknown>;
	outcome: FuzzOutcome;
}

/** Per-tool count summary. */
export interface FuzzToolSummary {
	tool: string;
	total: number;
	ok: number;
	protocol_error: number;
	runtime_error: number;
}

/** Top-level fuzz document. */
export interface FuzzResults {
	schema: typeof FUZZ_SCHEMA;
	scanned_at: string;
	server: { name?: string; version?: string };
	seed: number;
	budget: number;
	summary: FuzzToolSummary[];
	calls: FuzzCall[];
}
