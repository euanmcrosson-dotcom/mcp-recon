import { describe, expect, it } from "vitest";

import { extractToolFacts } from "../fuzz/schema.js";

describe("extractToolFacts", () => {
	it("returns isArgless=true for a tool with no inputSchema", () => {
		const f = extractToolFacts("noargs", null);
		expect(f.isArgless).toBe(true);
		expect(f.args).toEqual([]);
	});

	it("returns isArgless=true for empty properties", () => {
		const f = extractToolFacts("noargs", { type: "object", properties: {} });
		expect(f.isArgless).toBe(true);
	});

	it("extracts a single string arg", () => {
		const f = extractToolFacts("read", {
			type: "object",
			properties: { path: { type: "string" } },
			required: ["path"],
		});
		expect(f.isArgless).toBe(false);
		expect(f.args).toHaveLength(1);
		const arg = f.args[0];
		expect(arg).toBeDefined();
		if (!arg) return;
		expect(arg.path).toEqual(["path"]);
		expect(arg.declaredType).toBe("string");
		expect(arg.required).toBe(true);
		expect(arg.isPathShaped).toBe(true);
		expect(arg.isUrlShaped).toBe(false);
	});

	it("flags URL-shaped arg names", () => {
		const f = extractToolFacts("get", {
			type: "object",
			properties: { url: { type: "string" } },
			required: ["url"],
		});
		expect(f.args[0]?.isUrlShaped).toBe(true);
		expect(f.args[0]?.isPathShaped).toBe(false);
	});

	it("flags command-shaped arg names", () => {
		const f = extractToolFacts("exec", {
			type: "object",
			properties: { command: { type: "string" }, args: { type: "array" } },
			required: ["command"],
		});
		const argByName = (n: string) => f.args.find((a) => a.path[0] === n);
		expect(argByName("command")?.isCommandShaped).toBe(true);
		expect(argByName("args")?.isCommandShaped).toBe(true);
	});

	it("does not flag non-pathy names", () => {
		const f = extractToolFacts("get", {
			type: "object",
			properties: { id: { type: "string" }, message: { type: "string" } },
		});
		for (const a of f.args) {
			expect(a.isPathShaped).toBe(false);
			expect(a.isUrlShaped).toBe(false);
		}
	});

	it("handles unknown types gracefully", () => {
		const f = extractToolFacts("weird", {
			type: "object",
			properties: { thing: {} },
		});
		expect(f.args[0]?.declaredType).toBe("unknown");
	});

	it("captures enum values", () => {
		const f = extractToolFacts("e", {
			type: "object",
			properties: { mode: { type: "string", enum: ["a", "b", "c"] } },
		});
		expect(f.args[0]?.enumValues).toEqual(["a", "b", "c"]);
	});

	it("recognises union types — picks the first non-null", () => {
		const f = extractToolFacts("u", {
			type: "object",
			properties: { x: { type: ["null", "string"] } },
		});
		expect(f.args[0]?.declaredType).toBe("string");
	});

	// F006 regression — IANA-timezone-named args were misclassified as path-shaped
	// because the path-name list contains `source`/`target` and `source_timezone`
	// / `target_timezone` matched. Stop-word check in looksPathy now suppresses.
	describe("F006 regression — timezone-named args are NOT path-shaped", () => {
		it("source_timezone is not path-shaped", () => {
			const f = extractToolFacts("convert_time", {
				type: "object",
				properties: { source_timezone: { type: "string" } },
			});
			expect(f.args[0]?.isPathShaped).toBe(false);
		});

		it("target_timezone is not path-shaped", () => {
			const f = extractToolFacts("convert_time", {
				type: "object",
				properties: { target_timezone: { type: "string" } },
			});
			expect(f.args[0]?.isPathShaped).toBe(false);
		});

		it("plain `timezone` is not path-shaped", () => {
			const f = extractToolFacts("get_time", {
				type: "object",
				properties: { timezone: { type: "string" } },
			});
			expect(f.args[0]?.isPathShaped).toBe(false);
		});

		it("`tz` is not path-shaped", () => {
			const f = extractToolFacts("t", {
				type: "object",
				properties: { tz: { type: "string" } },
			});
			expect(f.args[0]?.isPathShaped).toBe(false);
		});

		it("region / locale / currency / country / language are not path-shaped", () => {
			for (const name of [
				"region",
				"locale",
				"currency",
				"country",
				"language",
			]) {
				const f = extractToolFacts("t", {
					type: "object",
					properties: { [name]: { type: "string" } },
				});
				expect(
					f.args[0]?.isPathShaped,
					`${name} should not be path-shaped`,
				).toBe(false);
			}
		});

		// Negative control — verify the path-shape detection still fires for real path args.
		it("plain `path` arg IS still path-shaped (positive control)", () => {
			const f = extractToolFacts("read", {
				type: "object",
				properties: { path: { type: "string" } },
			});
			expect(f.args[0]?.isPathShaped).toBe(true);
		});

		it("`source_path` IS still path-shaped (positive control)", () => {
			const f = extractToolFacts("read", {
				type: "object",
				properties: { source_path: { type: "string" } },
			});
			expect(f.args[0]?.isPathShaped).toBe(true);
		});
	});
});
