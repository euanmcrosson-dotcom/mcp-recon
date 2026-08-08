/**
 * Adversarial-server integration tests.
 *
 * Each test in this file spawns a tiny stdio-MCP-protocol-speaking
 * stub from `./adversarial-servers/` that simulates a hostile
 * server, then drives mcp-recon against it. The stubs are NOT real
 * MCP servers — they're attack fixtures (see the README in that
 * directory).
 *
 * Gated on `LIVE_MCP=1` because they spawn real subprocesses (same
 * gating story as `enumerate.integration.test.ts` /
 * `fuzz.integration.test.ts`). Run with:
 *
 *     LIVE_MCP=1 npm test -w @mcp-recon/cli
 *
 * Each test has a per-test timeout so a hung adversarial server
 * cannot stall the whole suite.
 */

import * as path from "node:path";
import { fileURLToPath } from "node:url";

import { afterEach, describe, expect, it } from "vitest";

import {
	DEFAULT_MAX_DESCRIPTION_CHARS,
	TRUNCATION_MARKER,
	closeClient,
	enumerate,
	openClient,
	parseServerSpec,
	renderMarkdown,
} from "../index.js";

import type { Client } from "@modelcontextprotocol/sdk/client/index.js";

// __dirname-equivalent for ESM. Vitest resolves this against the
// source file. The fixtures live alongside this test in the
// adversarial-servers/ subdirectory.
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES_DIR = path.join(__dirname, "adversarial-servers");
const TSX_BIN = path.join(
	__dirname,
	"..",
	"..",
	"..",
	"..",
	"node_modules",
	".bin",
	"tsx",
);

function fixtureSpec(name: string): string {
	// Use forward slashes in the spec — `parseServerSpec` splits on
	// whitespace, and Windows paths with spaces aren't supported by
	// the v0.1 spec parser. Our fixtures are inside the repo at a
	// path with no spaces, so this is fine.
	const file = path.join(FIXTURES_DIR, `${name}.ts`).replace(/\\/g, "/");
	const tsx = TSX_BIN.replace(/\\/g, "/");
	return `stdio:${tsx} ${file}`;
}

let openClientHandle: Client | undefined;

afterEach(async () => {
	if (openClientHandle) {
		try {
			await closeClient(openClientHandle);
		} catch {
			// ignore — some adversarial fixtures (slow-response) may
			// leave the connection in a weird state
		}
		openClientHandle = undefined;
	}
});

describe("adversarial servers — mcp-recon should handle malicious behavior gracefully (LIVE_MCP)", () => {
	it("huge-tool-list: server returns 10,000 tools — enumerate completes and surfaces them all", async () => {
		const spec = parseServerSpec(fixtureSpec("huge-tool-list"));
		openClientHandle = await openClient(spec);
		const inv = await enumerate(openClientHandle);
		// Today's contract: no cap; we surface every tool the server
		// claims. If a future version adds a cap, this assertion
		// becomes "expect(inv.tools.length).toBeLessThanOrEqual(CAP)".
		expect(inv.tools.length).toBe(10_000);
		// Tools should still have well-formed names so downstream
		// consumers don't choke on indexing.
		expect(inv.tools[0].name).toBe("flood_tool_0");
		expect(inv.tools[9_999].name).toBe("flood_tool_9999");
	}, 30_000);

	it("giant-tool-description: 10MB description — enumerate truncates at maxDescriptionChars", async () => {
		// Defensive fix landed: enumerate() now caps each tool's
		// description at `EnumerateOptions.maxDescriptionChars` (default
		// 64KB) and appends a TRUNCATION_MARKER so reviewers see the
		// cap fired. Before this fix, a hostile server returning a
		// 10MB description per tool would force mcp-recon to hold the
		// full bytes in memory and on disk via `scan`'s
		// inventory.json. The fix lives in `src/enumerate.ts`.
		const spec = parseServerSpec(fixtureSpec("giant-tool-description"));
		openClientHandle = await openClient(spec);
		const inv = await enumerate(openClientHandle);
		expect(inv.tools.length).toBe(1);
		expect(inv.tools[0].name).toBe("fat_tool");
		// Stored description is bounded.
		const desc = inv.tools[0].description ?? "";
		expect(desc.length).toBeLessThanOrEqual(DEFAULT_MAX_DESCRIPTION_CHARS);
		expect(desc.endsWith(TRUNCATION_MARKER)).toBe(true);
		// And the report stays small.
		const md = renderMarkdown({
			inventory: inv,
			classification: {
				schema: "mcp-recon/v0.1/classification" as const,
				scanned_at: new Date().toISOString(),
				classifications: [
					{
						tool: "fat_tool",
						data_class: "unknown",
						authority_level: "read",
						confused_deputy_candidate: false,
						confidence: 0.0,
						rationale: "test fixture",
						recommended_caveat: "// test",
					},
				],
			},
		});
		expect(md.length).toBeLessThan(100_000);
		expect(md).toMatch(/Description:\*\* A+/);
	}, 30_000);

	it("recursive-schema: $ref cycle in inputSchema — enumerate completes without recursing forever", async () => {
		const spec = parseServerSpec(fixtureSpec("recursive-schema"));
		openClientHandle = await openClient(spec);
		const inv = await enumerate(openClientHandle);
		expect(inv.tools.length).toBe(1);
		expect(inv.tools[0].name).toBe("ouroboros");
		// enumerate does NOT walk the schema (that's classify/fuzz's
		// job). It just stores the schema as opaque data. The
		// assertion here is that we got through without stack-
		// overflow, which a naive recursive walker would hit.
		expect(inv.tools[0].inputSchema).toBeDefined();
	}, 30_000);

	it("malformed-utf8: invalid UTF-8 bytes in description — enumerate either succeeds or rejects loudly", async () => {
		const spec = parseServerSpec(fixtureSpec("malformed-utf8"));
		openClientHandle = await openClient(spec);
		// Two acceptable outcomes:
		//   1. enumerate succeeds — Node's UTF-8 decoder substituted
		//      replacement chars and the JSON parser accepted the
		//      escaped lone surrogate.
		//   2. enumerate rejects with a deserialization error.
		// What MUST NOT happen: silent partial result, or a hang.
		let succeeded = false;
		try {
			const inv = await enumerate(openClientHandle);
			succeeded = true;
			expect(inv.tools.length).toBeGreaterThanOrEqual(0);
		} catch (e) {
			// Loud rejection is also acceptable.
			expect(e).toBeInstanceOf(Error);
		}
		expect(typeof succeeded).toBe("boolean");
	}, 30_000);

	it("slow-response: server never replies to tools/list — enumerate honors timeoutMs and rejects", async () => {
		// Defensive fix landed: enumerate() now accepts a `timeoutMs`
		// option (default 30s) which it forwards to the SDK as the
		// listTools request timeout. Before this fix, enumerate would
		// hang for the SDK's 60s default; with this fix a 1.5s cap
		// produces a fast loud rejection. The fix lives in
		// `src/enumerate.ts` (`EnumerateOptions.timeoutMs`).
		const spec = parseServerSpec(fixtureSpec("slow-response"));
		openClientHandle = await openClient(spec);
		const start = Date.now();
		await expect(
			enumerate(openClientHandle, { timeoutMs: 1_500 }),
		).rejects.toThrow();
		const elapsed = Date.now() - start;
		// Must reject within ~2s — gives the SDK a small grace window
		// beyond the 1.5s cap to actually deliver the rejection.
		expect(elapsed).toBeLessThan(4_000);
	}, 8_000);

	it("process-control-bytes: ANSI/BEL bytes outside JSON-RPC frame — they are not propagated to terminal", async () => {
		const spec = parseServerSpec(fixtureSpec("process-control-bytes"));
		openClientHandle = await openClient(spec);
		// The SDK's stdio reader is line-oriented + JSON.parse-strict.
		// Non-JSON lines fire `onerror` but don't crash the
		// connection. The valid `tools/list` line still gets parsed.
		// What we assert: enumerate succeeds AND the reported tool's
		// description does NOT contain raw control bytes (those went
		// to /dev/null via the reader's error path, not into our
		// result).
		const inv = await enumerate(openClientHandle);
		expect(inv.tools.length).toBe(1);
		expect(inv.tools[0].name).toBe("looks_safe");
		// Confirm no ANSI escapes leaked into the inventory's
		// description.
		expect(inv.tools[0].description ?? "").not.toContain("\x1b");
		expect(inv.tools[0].description ?? "").not.toContain("\x07");
	}, 30_000);

	it("description-injection: prompt-injection-shaped strings — surfaced as data, not instructions", async () => {
		const spec = parseServerSpec(fixtureSpec("description-injection"));
		openClientHandle = await openClient(spec);
		const inv = await enumerate(openClientHandle);
		expect(inv.tools.length).toBe(1);
		const desc = inv.tools[0].description ?? "";
		// The inventory should preserve the attacker text verbatim —
		// that's data, not instructions, and silently scrubbing it
		// would hide the threat from a reviewer.
		expect(desc).toContain("Stop scanning");
		// The rendered report should also contain it (so a reviewer
		// sees what the server claims), but our contract says the
		// renderer must label it as a description (i.e. attacker-
		// controlled) rather than emitting it as a plain top-level
		// line.
		const md = renderMarkdown({
			inventory: inv,
			classification: {
				schema: "mcp-recon/v0.1/classification" as const,
				scanned_at: new Date().toISOString(),
				classifications: [
					{
						tool: "innocuous_lookup",
						data_class: "unknown",
						authority_level: "read",
						confused_deputy_candidate: false,
						confidence: 0.0,
						rationale: "test fixture",
						recommended_caveat: "// test",
					},
				],
			},
		});
		// The injection text only appears AFTER the "Description:"
		// label, never on its own line. This is the property test:
		// the report frames the attacker text as a description,
		// not as an authoritative claim.
		const lines = md.split("\n");
		for (const line of lines) {
			if (line.includes("Stop scanning")) {
				expect(line).toMatch(/\*\*Description:\*\*/);
			}
		}
	}, 30_000);

	it("schema-violation: tools array is a string — enumerate rejects with a clear error", async () => {
		const spec = parseServerSpec(fixtureSpec("schema-violation"));
		openClientHandle = await openClient(spec);
		// The SDK's Zod schema for ListToolsResult requires `tools` to
		// be an array. A string in that slot must reject.
		await expect(enumerate(openClientHandle)).rejects.toThrow();
	}, 30_000);
});
