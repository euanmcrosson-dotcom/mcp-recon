/**
 * `fuzz` — runs the schema-aware adversarial fuzzer against an MCP
 * server and emits a v0.1 fuzz document.
 *
 * Per docs/SPEC.md §"Fuzzing strategy":
 *
 *   - Six axes (boundary / type-confusion / encoding / path-traversal /
 *     url-hostility / schema-violation)
 *   - Default budget: 200 calls per tool
 *   - Deterministic PRNG, default seed 0xC0FFEE
 *   - Records what happened — does NOT classify "success"
 *
 * The output JSON is consumed by the classifier (week 3) and the
 * reporter (week 3). Every entry has a `rationale` so a human
 * reviewer can grep for the interesting cases without re-running.
 */

import type { Client } from "@modelcontextprotocol/sdk/client/index.js";

import type { EnumeratedTool, ToolInventory } from "../enumerate.js";
import { generateBoundary } from "./axes/boundary.js";
import { generateEncoding } from "./axes/encoding.js";
import { generatePathTraversal } from "./axes/path-traversal.js";
import { generateSchemaViolation } from "./axes/schema-violation.js";
import { generateTypeConfusion } from "./axes/type-confusion.js";
import { generateUrlHostility } from "./axes/url-hostility.js";
import { Prng } from "./prng.js";
import {
	DEFAULT_FUZZ_BUDGET,
	DEFAULT_FUZZ_SEED,
	extractToolFacts,
} from "./schema.js";
import type { FuzzAxis, FuzzCall, FuzzOutcome, FuzzResults } from "./types.js";

export { FUZZ_SCHEMA } from "./types.js";
export type {
	FuzzAxis,
	FuzzCall,
	FuzzOutcome,
	FuzzResults,
	FuzzToolSummary,
} from "./types.js";

export interface FuzzOptions {
	/** Per-tool call budget. Default: 200. */
	budget?: number;
	/** PRNG seed for deterministic re-runs. Default: 0xC0FFEE. */
	seed?: number;
	/** If set, only fuzz these tool names (rest are skipped). */
	onlyTools?: readonly string[];
	/** Optional per-call timeout in ms. Default: 5000. */
	timeoutMs?: number;
}

/**
 * Run the fuzzer against `client`, given a previously-emitted
 * `inventory`. Returns a fuzz document.
 *
 * The `inventory` parameter (instead of re-enumerating internally)
 * is deliberate: the user can inspect / filter tools first, then
 * pass the resulting subset.
 */
export async function fuzz(
	client: Client,
	inventory: ToolInventory,
	options: FuzzOptions = {},
): Promise<FuzzResults> {
	const budget = options.budget ?? DEFAULT_FUZZ_BUDGET;
	const seed = options.seed ?? DEFAULT_FUZZ_SEED;
	const timeoutMs = options.timeoutMs ?? 5_000;
	const allowedTools = options.onlyTools ? new Set(options.onlyTools) : null;

	const calls: FuzzCall[] = [];
	const perTool: Record<
		string,
		{ ok: number; protocol_error: number; runtime_error: number }
	> = {};

	const prng = new Prng(seed);

	for (const tool of inventory.tools) {
		if (allowedTools && !allowedTools.has(tool.name)) continue;

		const facts = extractToolFacts(tool.name, tool.inputSchema);
		perTool[tool.name] = { ok: 0, protocol_error: 0, runtime_error: 0 };

		let calledForTool = 0;
		for (const { axis, args, rationale } of generateAllAxes(facts, prng)) {
			if (calledForTool >= budget) break;
			calledForTool++;

			const outcome = await callOnce(client, tool, args, timeoutMs);
			// perTool[tool.name] was assigned a few lines above before this
			// inner loop started; the index signature widens the type back
			// to `Tally | undefined`, but the value is provably present.
			// biome-ignore lint/style/noNonNullAssertion: see comment above
			const tally = perTool[tool.name]!;
			tally[outcome.kind]++;

			calls.push({
				tool: tool.name,
				axis,
				rationale,
				arguments: args,
				outcome,
			});
		}
	}

	return {
		schema: "mcp-recon/v0.1/fuzz",
		scanned_at: new Date().toISOString(),
		server: inventory.server,
		seed,
		budget,
		summary: Object.entries(perTool).map(([tool, t]) => ({
			tool,
			total: t.ok + t.protocol_error + t.runtime_error,
			ok: t.ok,
			protocol_error: t.protocol_error,
			runtime_error: t.runtime_error,
		})),
		calls,
	};
}

/** Concatenate all six axis-generators into one stream of (axis, input) pairs. */
function* generateAllAxes(
	facts: ReturnType<typeof extractToolFacts>,
	prng: Prng,
): Generator<{
	axis: FuzzAxis;
	args: Record<string, unknown>;
	rationale: string;
}> {
	for (const v of generateBoundary(facts, prng))
		yield { axis: "boundary_values", ...v };
	for (const v of generateTypeConfusion(facts, prng))
		yield { axis: "type_confusion", ...v };
	for (const v of generateEncoding(facts, prng))
		yield { axis: "encoding_tricks", ...v };
	for (const v of generatePathTraversal(facts, prng))
		yield { axis: "path_traversal", ...v };
	for (const v of generateUrlHostility(facts, prng))
		yield { axis: "url_hostility", ...v };
	for (const v of generateSchemaViolation(facts, prng))
		yield { axis: "schema_violation", ...v };
}

async function callOnce(
	client: Client,
	tool: EnumeratedTool,
	args: Record<string, unknown>,
	timeoutMs: number,
): Promise<FuzzOutcome> {
	// Race the call against a timeout so a hanging server doesn't
	// freeze the whole fuzz run.
	let timeoutHandle: ReturnType<typeof setTimeout> | undefined;
	const timeout = new Promise<FuzzOutcome>((resolve) => {
		timeoutHandle = setTimeout(() => {
			resolve({
				kind: "runtime_error",
				message: `timeout after ${timeoutMs}ms`,
			});
		}, timeoutMs);
	});

	const call = (async (): Promise<FuzzOutcome> => {
		try {
			const result = await client.callTool({
				name: tool.name,
				arguments: args,
			});
			// The MCP SDK returns `{ content, isError }`. `isError === true`
			// means the tool returned a structured error — that's a
			// protocol error in our taxonomy (the tool said "no" cleanly).
			const r = result as { content?: unknown; isError?: boolean };
			if (r.isError) {
				return {
					kind: "protocol_error",
					message: snippet(JSON.stringify(r.content ?? "")),
				};
			}
			return { kind: "ok", snippet: snippet(JSON.stringify(r.content ?? "")) };
		} catch (err) {
			// SDK threw — usually a transport-level or JSON-RPC-level
			// error. We treat MCP-level error responses as `protocol_error`
			// and everything else as `runtime_error`. The SDK throws an
			// `McpError` for protocol errors; in lieu of import-time
			// coupling to that class, we string-match on the message.
			const message = err instanceof Error ? err.message : String(err);
			const isProtocolError =
				/MCP error|JSON-RPC|invalid params|method not found|invalid request/i.test(
					message,
				);
			return isProtocolError
				? { kind: "protocol_error", message: snippet(message) }
				: { kind: "runtime_error", message: snippet(message) };
		}
	})();

	try {
		return await Promise.race([call, timeout]);
	} finally {
		if (timeoutHandle !== undefined) clearTimeout(timeoutHandle);
	}
}

function snippet(s: string, max = 200): string {
	if (s.length <= max) return s;
	return `${s.slice(0, max)}…[len=${s.length}]`;
}
