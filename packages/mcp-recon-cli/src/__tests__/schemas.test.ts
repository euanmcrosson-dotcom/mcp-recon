/**
 * Tests for the published JSON Schemas in `/schemas/` at the repo root.
 *
 * These tests are the wire-format contract enforcer: every published
 * example artifact must validate against its corresponding JSON
 * Schema, and a deliberately-mutated example must fail. Together
 * those two assertions prove the schema accepts the documents the
 * tool actually emits and rejects shapes it does not.
 */

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import addFormats from "ajv-formats";
import Ajv2020 from "ajv/dist/2020.js";
import { describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));
// __tests__ → src → mcp-recon-cli → packages → repo root
const REPO_ROOT = resolve(__dirname, "..", "..", "..", "..");

function loadJson(relPath: string): unknown {
	return JSON.parse(readFileSync(resolve(REPO_ROOT, relPath), "utf8"));
}

function compile(schemaPath: string) {
	const schema = loadJson(schemaPath);
	const ajv = new Ajv2020({ strict: false, allErrors: true });
	addFormats(ajv);
	return ajv.compile(schema as object);
}

const SCHEMAS = {
	inventory: "schemas/inventory.v0.1.json",
	fuzz: "schemas/fuzz.v0.1.json",
	classification: "schemas/classification.v0.1.json",
	caveats: "schemas/caveats.v0.1.json",
} as const;

const EXAMPLES = {
	inventory: "examples/public-servers/server-filesystem/inventory.json",
	fuzz: "examples/public-servers/server-filesystem/fuzz.json",
	classification:
		"examples/public-servers/server-filesystem/classification.json",
} as const;

describe("schemas/*.v0.1.json — schema files are themselves valid JSON Schemas", () => {
	it.each(Object.entries(SCHEMAS))(
		"compiles %s schema with Ajv 2020-12",
		(_name, path) => {
			expect(() => compile(path)).not.toThrow();
		},
	);
});

describe("schemas/inventory.v0.1.json", () => {
	const validate = compile(SCHEMAS.inventory);
	const example = loadJson(EXAMPLES.inventory) as Record<string, unknown>;

	it("validates the published filesystem-server inventory example", () => {
		const ok = validate(example);
		expect(validate.errors ?? null).toBeNull();
		expect(ok).toBe(true);
	});

	it("rejects an inventory missing `tools`", () => {
		const { tools, ...broken } = example;
		void tools;
		expect(validate(broken)).toBe(false);
	});

	it("rejects a wrong schema-tag string", () => {
		const broken = { ...example, schema: "mcp-recon/v0.2/inventory" };
		expect(validate(broken)).toBe(false);
	});
});

describe("schemas/fuzz.v0.1.json", () => {
	const validate = compile(SCHEMAS.fuzz);
	const example = loadJson(EXAMPLES.fuzz) as Record<string, unknown>;

	it("validates the published filesystem-server fuzz example", () => {
		const ok = validate(example);
		expect(validate.errors ?? null).toBeNull();
		expect(ok).toBe(true);
	});

	it("rejects a fuzz call with an unknown axis", () => {
		const broken = JSON.parse(JSON.stringify(example));
		broken.calls[0].axis = "not_a_real_axis";
		expect(validate(broken)).toBe(false);
	});

	it("rejects a fuzz call with an unknown outcome.kind", () => {
		const broken = JSON.parse(JSON.stringify(example));
		broken.calls[0].outcome = { kind: "exploded", message: "boom" };
		expect(validate(broken)).toBe(false);
	});
});

describe("schemas/classification.v0.1.json", () => {
	const validate = compile(SCHEMAS.classification);
	const example = loadJson(EXAMPLES.classification) as Record<string, unknown>;

	it("validates the published filesystem-server classification example", () => {
		const ok = validate(example);
		expect(validate.errors ?? null).toBeNull();
		expect(ok).toBe(true);
	});

	it("rejects a confidence value outside [0, 1]", () => {
		const broken = JSON.parse(JSON.stringify(example));
		broken.classifications[0].confidence = 1.5;
		expect(validate(broken)).toBe(false);
	});

	it("rejects an unknown data_class", () => {
		const broken = JSON.parse(JSON.stringify(example));
		broken.classifications[0].data_class = "blockchain";
		expect(validate(broken)).toBe(false);
	});

	it("rejects a missing required field (recommended_caveat)", () => {
		const broken = JSON.parse(JSON.stringify(example));
		broken.classifications[0].recommended_caveat = undefined;
		expect(validate(broken)).toBe(false);
	});
});

describe("schemas/caveats.v0.1.json", () => {
	const validate = compile(SCHEMAS.caveats);

	// No published example yet (the `caveats` command lands in PR #1).
	// Synthesise a minimal valid document that mirrors the shape
	// `planCaveats()` produces against the filesystem-server
	// classification with sample bindings. This is the same shape
	// documented in `packages/mcp-recon-cli/src/caveats/types.ts` on
	// `feat/caveats-command`.
	const example = {
		schema: "mcp-recon/v0.1/caveats",
		scanned_at: "2026-04-30T12:00:00.000Z",
		server: { name: "secure-filesystem-server", version: "0.2.0" },
		bindings: {
			caller: "agent-sandbox-1",
			sandbox_prefix: "/tmp/sandbox",
			expiry: "2026-05-01T00:00:00Z",
		},
		plans: [
			{
				tool: "read_file",
				data_class: "filesystem",
				authority_level: "read",
				confused_deputy_candidate: false,
				purpose: "READ filesystem",
				caveats: [
					'tool == "read_file"',
					'caller == "agent-sandbox-1"',
					'arg.path starts_with "/tmp/sandbox/"',
					"now <= @2026-05-01T00:00:00Z",
				],
				flagged: false,
				flag_reasons: [],
				comment: "READ filesystem; bound the sandbox prefix tightly",
			},
			{
				tool: "write_file",
				data_class: "filesystem",
				authority_level: "write",
				confused_deputy_candidate: true,
				purpose: "WRITE filesystem",
				caveats: [
					'tool == "write_file"',
					'arg.path starts_with "/tmp/sandbox/"',
				],
				flagged: true,
				flag_reasons: ["unsubstituted_placeholder"],
			},
		],
		summary: { total: 2, ready: 1, flagged: 1 },
	} as const;

	it("validates a synthesised caveats document with mixed flagged/ready plans", () => {
		const ok = validate(example);
		expect(validate.errors ?? null).toBeNull();
		expect(ok).toBe(true);
	});

	it("rejects a caveats document with an unknown flag_reason", () => {
		const broken = JSON.parse(JSON.stringify(example));
		broken.plans[1].flag_reasons.push("operator_was_grumpy");
		expect(validate(broken)).toBe(false);
	});

	it("rejects a caveats plan missing the required `caveats` array", () => {
		const broken = JSON.parse(JSON.stringify(example));
		broken.plans[0].caveats = undefined;
		expect(validate(broken)).toBe(false);
	});

	it("rejects a caveats document with the wrong schema tag", () => {
		const broken = JSON.parse(JSON.stringify(example));
		broken.schema = "mcp-recon/v0.1/classification";
		expect(validate(broken)).toBe(false);
	});
});
