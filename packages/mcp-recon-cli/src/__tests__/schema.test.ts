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
});
