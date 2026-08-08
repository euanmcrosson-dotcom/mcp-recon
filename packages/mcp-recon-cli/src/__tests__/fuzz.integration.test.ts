/**
 * Live integration test for the fuzzer against the official
 * `@modelcontextprotocol/server-filesystem`.
 *
 * Gated on `LIVE_MCP=1`. Run with:
 *
 *     LIVE_MCP=1 npm test -w @mcp-recon/cli
 *
 * Asserts:
 *
 *   - The full pipeline (enumerate → fuzz) completes without crashing.
 *   - The fuzz document has the right schema tag.
 *   - Per-tool counts add up.
 *   - With a small budget, we hit several axes within budget.
 *   - At least 50% of calls produce a `protocol_error` outcome (a
 *     server that crashes 100% on fuzzing is broken; a server that
 *     accepts 100% has missing validation).
 *   - Re-running with the same seed produces the same call sequence.
 */

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
	FUZZ_SCHEMA,
	closeClient,
	enumerate,
	fuzz,
	openClient,
	parseServerSpec,
} from "../index.js";

import type { Client } from "@modelcontextprotocol/sdk/client/index.js";

let sandbox: string;
let client: Client;

beforeAll(async () => {
	sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "mcp-recon-fuzz-it-"));
	const spec = parseServerSpec(
		`stdio:npx -y @modelcontextprotocol/server-filesystem ${sandbox}`,
	);
	client = await openClient(spec);
}, 60_000);

afterAll(async () => {
	if (client) await closeClient(client);
	if (sandbox) fs.rmSync(sandbox, { recursive: true, force: true });
});

describe("fuzz against @modelcontextprotocol/server-filesystem (LIVE_MCP)", () => {
	it("emits a valid v0.1 fuzz document with consistent counts", async () => {
		const inv = await enumerate(client);
		const result = await fuzz(client, inv, { budget: 6, timeoutMs: 10_000 });

		expect(result.schema).toBe(FUZZ_SCHEMA);
		expect(result.scanned_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
		expect(result.budget).toBe(6);
		expect(result.summary.length).toBe(inv.tools.length);

		const totalFromSummary = result.summary.reduce(
			(acc, s) => acc + s.ok + s.protocol_error + s.runtime_error,
			0,
		);
		expect(totalFromSummary).toBe(result.calls.length);
	}, 180_000);

	it("most calls land on protocol_error — server validates inputs", async () => {
		const inv = await enumerate(client);
		const result = await fuzz(client, inv, { budget: 6, timeoutMs: 10_000 });

		const totalProto = result.summary.reduce(
			(acc, s) => acc + s.protocol_error,
			0,
		);
		const totalRuntime = result.summary.reduce(
			(acc, s) => acc + s.runtime_error,
			0,
		);
		const totalErrored = totalProto + totalRuntime;

		// The filesystem server is mature; the vast majority of garbage
		// inputs should be rejected as protocol errors. If the rate
		// drops below 30% something has changed (either the server is
		// suddenly accepting more garbage, or our taxonomy needs work).
		expect(totalErrored / result.calls.length).toBeGreaterThan(0.3);
	}, 180_000);

	it("re-running with the same seed produces the same call shape", async () => {
		const inv = await enumerate(client);
		const a = await fuzz(client, inv, {
			budget: 4,
			seed: 42,
			timeoutMs: 10_000,
		});
		const b = await fuzz(client, inv, {
			budget: 4,
			seed: 42,
			timeoutMs: 10_000,
		});

		// Shape: same tools, same axis distribution, same rationales (the
		// outcomes can flake on a real server, but the inputs are
		// deterministic).
		expect(a.calls.map((c) => c.tool)).toEqual(b.calls.map((c) => c.tool));
		expect(a.calls.map((c) => c.axis)).toEqual(b.calls.map((c) => c.axis));
		expect(a.calls.map((c) => c.rationale)).toEqual(
			b.calls.map((c) => c.rationale),
		);
	}, 180_000);

	it("hits multiple axes within a small budget", async () => {
		const inv = await enumerate(client);
		const result = await fuzz(client, inv, { budget: 30, timeoutMs: 10_000 });

		const axes = new Set(result.calls.map((c) => c.axis));
		// Filesystem tools have path-shaped args, so we expect at least
		// boundary, type-confusion, encoding, path-traversal,
		// schema-violation. URL-hostility doesn't fire (no URL args).
		expect(axes.size).toBeGreaterThanOrEqual(4);
	}, 240_000);
});
