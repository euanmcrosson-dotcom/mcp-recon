/**
 * JSON Schema walker — extracts what the fuzzer needs to know about a
 * tool's argument shape.
 *
 * v0.1 supports JSON Schema Draft 7 (the dialect the official MCP
 * filesystem server uses). We don't validate the schema — we extract
 * structural facts: which keys exist, what types they declare, which
 * are required, which look like paths or URLs.
 *
 * Heuristic shape-detection: arg names containing `path` are flagged
 * path-shaped; arg names containing `url`, `origin`, `endpoint` are
 * URL-shaped. The classifier (week 3) will refine these heuristics.
 */

/** Default seed for the deterministic fuzz PRNG (matches docs/SPEC.md). */
export const DEFAULT_FUZZ_SEED = 0xc0_ffee;

/** Default per-tool fuzz budget (matches docs/SPEC.md). */
export const DEFAULT_FUZZ_BUDGET = 200;

/** Recognised JSON Schema primitive types we generate inputs for. */
export type SchemaType =
	| "string"
	| "number"
	| "integer"
	| "boolean"
	| "array"
	| "object"
	| "null";

export interface ToolArgFact {
	/** Path within the args object — e.g. ["path"], ["options", "recursive"]. */
	path: string[];
	/** Declared JSON Schema type. `unknown` if the schema didn't say. */
	declaredType: SchemaType | "unknown";
	/** Whether this arg is required (per the schema's `required` array). */
	required: boolean;
	/** Heuristic: is this arg name path-shaped (filesystem)? */
	isPathShaped: boolean;
	/** Heuristic: is this arg name URL-shaped (network)? */
	isUrlShaped: boolean;
	/** Heuristic: is this arg name command-shaped (shell)? */
	isCommandShaped: boolean;
	/** If declared as enum, the allowed values. */
	enumValues?: readonly unknown[];
}

export interface ToolFacts {
	/** Tool name. */
	name: string;
	/** All discovered arg facts (top-level only in v0.1). */
	args: ToolArgFact[];
	/** Whether the schema declares no args at all (an args-less tool). */
	isArgless: boolean;
}

/** Walk one tool's `inputSchema`. Returns structural facts. */
export function extractToolFacts(
	name: string,
	inputSchema: unknown,
): ToolFacts {
	if (inputSchema === null || typeof inputSchema !== "object") {
		return { name, args: [], isArgless: true };
	}
	const schema = inputSchema as Record<string, unknown>;
	const properties = (schema.properties ?? {}) as Record<string, unknown>;
	const requiredList = Array.isArray(schema.required)
		? (schema.required as string[])
		: [];
	const requiredSet = new Set(requiredList);

	const args: ToolArgFact[] = [];
	for (const [argName, argSchema] of Object.entries(properties)) {
		if (argSchema === null || typeof argSchema !== "object") continue;
		const a = argSchema as Record<string, unknown>;
		const declaredType = pickType(a.type);
		const enumRaw = a.enum;
		const enumValues = Array.isArray(enumRaw)
			? (enumRaw as readonly unknown[])
			: undefined;

		args.push({
			path: [argName],
			declaredType,
			required: requiredSet.has(argName),
			isPathShaped: looksPathy(argName),
			isUrlShaped: looksUrly(argName),
			isCommandShaped: looksCommandy(argName),
			...(enumValues ? { enumValues } : {}),
		});
	}
	return { name, args, isArgless: args.length === 0 };
}

function pickType(declared: unknown): SchemaType | "unknown" {
	if (typeof declared === "string") {
		if (
			declared === "string" ||
			declared === "number" ||
			declared === "integer" ||
			declared === "boolean" ||
			declared === "array" ||
			declared === "object" ||
			declared === "null"
		) {
			return declared;
		}
	}
	if (Array.isArray(declared) && declared.length > 0) {
		// Multi-type arg (`type: ["string", "null"]`) — pick the first non-null.
		const first = declared.find((t) => t !== "null") ?? declared[0];
		if (typeof first === "string") return pickType(first);
	}
	return "unknown";
}

function looksPathy(argName: string): boolean {
	// Suppress when the arg name carries a non-path semantic that the
	// path-name list (`source`, `target`, `destination`, ...) would otherwise
	// false-positive on. Canonical case: `source_timezone` / `target_timezone`
	// on `mcp-server-time` (F006). Add additional stop words here as new
	// false-positive shapes surface.
	if (
		/(^|_)(timezone|tz|zone|region|locale|currency|country|language)($|_|s$)/i.test(
			argName,
		)
	) {
		return false;
	}
	return /(^|_)(path|file|dir|directory|src|source|destination|dest|target)($|_|s$)/i.test(
		argName,
	);
}

function looksUrly(argName: string): boolean {
	return /(^|_)(url|uri|origin|endpoint|host|hostname)($|_|s$)/i.test(argName);
}

function looksCommandy(argName: string): boolean {
	return /(^|_)(cmd|command|argv|exec|args)($|_|s$)/i.test(argName);
}
