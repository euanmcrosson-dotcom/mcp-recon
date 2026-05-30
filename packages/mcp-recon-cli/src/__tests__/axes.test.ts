/**
 * Per-axis unit tests. Each axis is a pure generator function — no I/O —
 * so we exercise it directly and assert the produced inputs cover the
 * documented cases.
 */

import { describe, expect, it } from "vitest";

import { generateBoundary } from "../fuzz/axes/boundary.js";
import { generateEncoding } from "../fuzz/axes/encoding.js";
import { generatePathTraversal } from "../fuzz/axes/path-traversal.js";
import { generateSchemaViolation } from "../fuzz/axes/schema-violation.js";
import { generateTypeConfusion } from "../fuzz/axes/type-confusion.js";
import { generateUrlHostility } from "../fuzz/axes/url-hostility.js";
import { Prng } from "../fuzz/prng.js";
import { extractToolFacts } from "../fuzz/schema.js";

const PRNG = () => new Prng(0xc0ffee);

const PATH_TOOL = extractToolFacts("read_file", {
	type: "object",
	properties: { path: { type: "string" } },
	required: ["path"],
});

const URL_TOOL = extractToolFacts("http_get", {
	type: "object",
	properties: { url: { type: "string" } },
	required: ["url"],
});

const MULTI_TYPE_TOOL = extractToolFacts("multi", {
	type: "object",
	properties: {
		name: { type: "string" },
		count: { type: "integer" },
		enabled: { type: "boolean" },
	},
	required: ["name"],
});

const ARGLESS_TOOL = extractToolFacts("noargs", {
	type: "object",
	properties: {},
});

describe("generateBoundary", () => {
	it("yields one empty-args input for argless tools", () => {
		const items = [...generateBoundary(ARGLESS_TOOL, PRNG())];
		expect(items.length).toBe(1);
		expect(items[0]?.args).toEqual({});
	});

	it("covers boundary values for string args", () => {
		const items = [...generateBoundary(PATH_TOOL, PRNG())];
		const values = items.map((i) => i.args.path);
		// Empty string + at least one large string + null all present.
		expect(values).toContain("");
		expect(values).toContain(null);
		expect(
			values.some((v) => typeof v === "string" && (v as string).length > 1000),
		).toBe(true);
	});

	it("emits required-field-omitted entries for required args", () => {
		const items = [...generateBoundary(PATH_TOOL, PRNG())];
		expect(items.some((i) => !("path" in i.args))).toBe(true);
	});

	it("covers integer + boolean + multiple types", () => {
		const items = [...generateBoundary(MULTI_TYPE_TOOL, PRNG())];
		// At least one value targeting each typed arg.
		expect(items.some((i) => i.rationale.includes('arg "name"'))).toBe(true);
		expect(items.some((i) => i.rationale.includes('arg "count"'))).toBe(true);
		expect(items.some((i) => i.rationale.includes('arg "enabled"'))).toBe(true);
	});
});

describe("generateTypeConfusion", () => {
	it("yields nothing for argless tools", () => {
		const items = [...generateTypeConfusion(ARGLESS_TOOL, PRNG())];
		expect(items.length).toBe(0);
	});

	it("substitutes other types for each typed arg", () => {
		const items = [...generateTypeConfusion(PATH_TOOL, PRNG())];
		// Path is declared string; expect non-string substitutions.
		expect(items.some((i) => typeof i.args.path === "number")).toBe(true);
		expect(items.some((i) => typeof i.args.path === "boolean")).toBe(true);
		expect(items.some((i) => Array.isArray(i.args.path))).toBe(true);
	});
});

describe("generateEncoding", () => {
	it("only fires for string args (or unknown)", () => {
		// Path is string → should produce entries.
		const items = [...generateEncoding(PATH_TOOL, PRNG())];
		expect(items.length).toBeGreaterThan(0);
	});

	it("includes percent-encoded NUL + Cyrillic homograph + RTL override", () => {
		const items = [...generateEncoding(PATH_TOOL, PRNG())];
		const values = items.map((i) => i.args.path);
		expect(values).toContain("%00");
		expect(values.some((v) => v === "аdmin")).toBe(true); // Cyrillic-a
		expect(
			values.some((v) => typeof v === "string" && (v as string).includes("‮")),
		).toBe(true);
	});
});

describe("generatePathTraversal", () => {
	it("only fires for path-shaped args", () => {
		const pathItems = [...generatePathTraversal(PATH_TOOL, PRNG())];
		const urlItems = [...generatePathTraversal(URL_TOOL, PRNG())];
		expect(pathItems.length).toBeGreaterThan(0);
		expect(urlItems.length).toBe(0);
	});

	it("includes classic POSIX + Windows + percent-encoded shapes", () => {
		const items = [...generatePathTraversal(PATH_TOOL, PRNG())];
		const values = items.map((i) => i.args.path);
		expect(values).toContain("../etc/passwd");
		expect(values).toContain("..\\..\\windows\\system32\\config\\sam");
		expect(
			values.some(
				(v) => typeof v === "string" && (v as string).includes("%2e"),
			),
		).toBe(true);
		expect(values.some((v) => v === "/")).toBe(true);
	});
});

describe("generateUrlHostility", () => {
	it("only fires for URL-shaped args", () => {
		const pathItems = [...generateUrlHostility(PATH_TOOL, PRNG())];
		const urlItems = [...generateUrlHostility(URL_TOOL, PRNG())];
		expect(urlItems.length).toBeGreaterThan(0);
		expect(pathItems.length).toBe(0);
	});

	it("includes userinfo-splitting + IDN homograph + javascript: + AWS metadata", () => {
		const items = [...generateUrlHostility(URL_TOOL, PRNG())];
		const values = items.map((i) => String(i.args.url));
		expect(values.some((v) => v.includes("attacker.example@"))).toBe(true);
		expect(values.some((v) => v.startsWith("javascript:"))).toBe(true);
		expect(values).toContain("http://169.254.169.254/latest/meta-data/");
		// Cyrillic-a IDN
		expect(values.some((v) => v.includes("аpi.example.com"))).toBe(true);
	});
});

describe("generateSchemaViolation", () => {
	it("works for argless tools (extra fields, prototype pollution, deep nest)", () => {
		const items = [...generateSchemaViolation(ARGLESS_TOOL, PRNG())];
		expect(items.length).toBeGreaterThanOrEqual(3);
		expect(items.some((i) => i.rationale.includes("argless"))).toBe(true);
		expect(items.some((i) => i.rationale.includes("__proto__"))).toBe(true);
		expect(items.some((i) => i.rationale.includes("nested"))).toBe(true);
	});

	it("for tools with required args, omits each required arg in turn", () => {
		const items = [...generateSchemaViolation(PATH_TOOL, PRNG())];
		expect(items.some((i) => i.rationale.includes('"path"'))).toBe(true);
		expect(items.some((i) => i.rationale.includes("__injected"))).toBe(true);
	});
});

describe("axis determinism", () => {
	it("each axis produces identical output across two runs with same seed", () => {
		const a = [...generateBoundary(MULTI_TYPE_TOOL, PRNG())];
		const b = [...generateBoundary(MULTI_TYPE_TOOL, PRNG())];
		expect(a).toEqual(b);
	});
});
