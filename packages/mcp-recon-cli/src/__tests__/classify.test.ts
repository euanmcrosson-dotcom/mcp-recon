import { describe, expect, it } from "vitest";

import { classify, noisyOr, synthesizeCaveat } from "../classify/index.js";
import type { ToolInventory } from "../enumerate.js";
import { extractToolFacts } from "../fuzz/schema.js";

const FROZEN_NOW = "2026-04-29T12:00:00.000Z";

function inv(tools: ToolInventory["tools"]): ToolInventory {
	return {
		schema: "mcp-recon/v0.1/inventory",
		scanned_at: FROZEN_NOW,
		server: { name: "test-server", version: "1.0.0" },
		tools,
	};
}

describe("noisyOr", () => {
	it("empty input → 0", () => {
		expect(noisyOr([])).toBe(0);
	});

	it("single value passes through", () => {
		expect(noisyOr([0.7])).toBeCloseTo(0.7, 5);
	});

	it("two equal weights combine super-additively but capped", () => {
		// 1 - (1-0.5)*(1-0.5) = 0.75
		expect(noisyOr([0.5, 0.5])).toBeCloseTo(0.75, 5);
	});

	it("clamps inputs to [0, 1]", () => {
		expect(noisyOr([-0.5])).toBe(0);
		expect(noisyOr([1.5])).toBeCloseTo(1, 5);
	});

	it("never exceeds 1", () => {
		const result = noisyOr([0.9, 0.9, 0.9, 0.9]);
		expect(result).toBeGreaterThan(0.99);
		expect(result).toBeLessThanOrEqual(1);
	});
});

describe("classify — filesystem rules", () => {
	it("read_file → filesystem/read with confused-deputy=false (read authority)", () => {
		const result = classify(
			inv([
				{
					name: "read_file",
					description: "Read the contents of a file at a given path.",
					inputSchema: {
						type: "object",
						properties: { path: { type: "string" } },
						required: ["path"],
					},
				},
			]),
		);
		const c = result.classifications[0];
		expect(c).toBeDefined();
		if (!c) return;
		expect(c.data_class).toBe("filesystem");
		expect(c.authority_level).toBe("read");
		expect(c.confused_deputy_candidate).toBe(false);
		expect(c.confidence).toBeGreaterThan(0.7);
	});

	it("write_file → filesystem/write with confused-deputy=true", () => {
		const result = classify(
			inv([
				{
					name: "write_file",
					description:
						"Write content to a file at a given path. Creates the file if it doesn't exist.",
					inputSchema: {
						type: "object",
						properties: {
							path: { type: "string" },
							content: { type: "string" },
						},
						required: ["path", "content"],
					},
				},
			]),
		);
		const c = result.classifications[0];
		expect(c).toBeDefined();
		if (!c) return;
		expect(c.data_class).toBe("filesystem");
		expect(c.authority_level).toBe("write");
		expect(c.confused_deputy_candidate).toBe(true);
	});

	it("delete_path → filesystem/destructive with confused-deputy=true", () => {
		const result = classify(
			inv([
				{
					name: "delete_path",
					description: "Delete the file or directory at the given path.",
					inputSchema: {
						type: "object",
						properties: { path: { type: "string" } },
						required: ["path"],
					},
				},
			]),
		);
		const c = result.classifications[0];
		expect(c).toBeDefined();
		if (!c) return;
		expect(c.data_class).toBe("filesystem");
		expect(c.authority_level).toBe("destructive");
		expect(c.confused_deputy_candidate).toBe(true);
	});
});

describe("classify — network + shell + payments", () => {
	it("http_get → network/read with URL-shaped arg detected", () => {
		const result = classify(
			inv([
				{
					name: "http_get",
					description:
						"Make an HTTP GET request to the given URL and return the response.",
					inputSchema: {
						type: "object",
						properties: { url: { type: "string" } },
						required: ["url"],
					},
				},
			]),
		);
		const c = result.classifications[0];
		expect(c).toBeDefined();
		if (!c) return;
		expect(c.data_class).toBe("network");
		expect(c.rationale).toContain("URL-shaped");
	});

	it("shell_exec → shell/privileged (regardless of side-effect verbs)", () => {
		const result = classify(
			inv([
				{
					name: "shell_exec",
					description: "Run a command in a subprocess.",
					inputSchema: {
						type: "object",
						properties: { command: { type: "string" } },
						required: ["command"],
					},
				},
			]),
		);
		const c = result.classifications[0];
		expect(c).toBeDefined();
		if (!c) return;
		expect(c.data_class).toBe("shell");
		expect(c.authority_level).toBe("privileged");
		expect(c.confused_deputy_candidate).toBe(true);
	});

	it("charge_card → payments/write", () => {
		const result = classify(
			inv([
				{
					name: "charge_card",
					description: "Charge a credit card for the given amount.",
					inputSchema: {
						type: "object",
						properties: {
							card: { type: "string" },
							amount: { type: "number" },
						},
						required: ["card", "amount"],
					},
				},
			]),
		);
		const c = result.classifications[0];
		expect(c).toBeDefined();
		if (!c) return;
		expect(c.data_class).toBe("payments");
		expect(c.authority_level).toBeDefined();
	});
});

describe("classify — unknown handling", () => {
	it("opaque tool name + no useful description → unknown", () => {
		const result = classify(
			inv([
				{
					name: "do_thing",
					description: "Does a thing.",
					inputSchema: { type: "object", properties: {} },
				},
			]),
		);
		const c = result.classifications[0];
		expect(c).toBeDefined();
		if (!c) return;
		expect(c.data_class).toBe("unknown");
		expect(c.confidence).toBe(0);
	});
});

describe("classify — fuzz informed", () => {
	it("fuzz_informed=true when fuzz results are passed", () => {
		const inventory = inv([
			{
				name: "read_file",
				description: "Read a file.",
				inputSchema: {
					type: "object",
					properties: { path: { type: "string" } },
					required: ["path"],
				},
			},
		]);
		const fuzz = {
			schema: "mcp-recon/v0.1/fuzz" as const,
			scanned_at: FROZEN_NOW,
			server: inventory.server,
			seed: 0xc0_ffee,
			budget: 10,
			summary: [
				{
					tool: "read_file",
					total: 5,
					ok: 1,
					protocol_error: 4,
					runtime_error: 0,
				},
			],
			calls: [],
		};
		const a = classify(inventory);
		const b = classify(inventory, fuzz);

		expect(a.fuzz_informed).toBe(false);
		expect(b.fuzz_informed).toBe(true);
		// Fuzz with at least one ok response raises confidence.
		expect(b.classifications[0]?.confidence ?? 0).toBeGreaterThan(
			a.classifications[0]?.confidence ?? 0,
		);
	});
});

describe("synthesizeCaveat", () => {
	it("privileged → deny suggestion", () => {
		const facts = extractToolFacts("shell_exec", {
			type: "object",
			properties: { command: { type: "string" } },
			required: ["command"],
		});
		const caveat = synthesizeCaveat({
			tool: "shell_exec",
			data_class: "shell",
			authority_level: "privileged",
			facts,
		});
		expect(caveat).toContain("PRIVILEGED");
		expect(caveat).toContain('tool != "shell_exec"');
	});

	it("filesystem read → starts_with prefix caveat", () => {
		const facts = extractToolFacts("read_file", {
			type: "object",
			properties: { path: { type: "string" } },
			required: ["path"],
		});
		const caveat = synthesizeCaveat({
			tool: "read_file",
			data_class: "filesystem",
			authority_level: "read",
			facts,
		});
		expect(caveat).toContain('tool == "read_file"');
		expect(caveat).toContain("arg.path starts_with");
		expect(caveat).toContain("now <=");
	});

	it("network → origin equality caveat", () => {
		const facts = extractToolFacts("http_get", {
			type: "object",
			properties: { url: { type: "string" } },
			required: ["url"],
		});
		const caveat = synthesizeCaveat({
			tool: "http_get",
			data_class: "network",
			authority_level: "read",
			facts,
		});
		expect(caveat).toContain("arg.url ==");
	});
});
