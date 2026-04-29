import { describe, expect, it } from "vitest";

import { classify } from "../classify/index.js";
import { renderMarkdown } from "../report/index.js";
import type { ToolInventory } from "../enumerate.js";

const FROZEN_NOW = "2026-04-29T12:00:00.000Z";

const SAMPLE_INV: ToolInventory = {
  schema: "mcp-recon/v0.1/inventory",
  scanned_at: FROZEN_NOW,
  server: { name: "sample-server", version: "0.1.0" },
  tools: [
    {
      name: "read_file",
      description: "Read a file.",
      inputSchema: { type: "object", properties: { path: { type: "string" } }, required: ["path"] },
    },
    {
      name: "delete_path",
      description: "Delete the file or directory at path.",
      inputSchema: { type: "object", properties: { path: { type: "string" } }, required: ["path"] },
    },
    {
      name: "shell_exec",
      description: "Execute a shell command.",
      inputSchema: { type: "object", properties: { command: { type: "string" } }, required: ["command"] },
    },
  ],
};

describe("renderMarkdown", () => {
  it("renders a heading with server identity", () => {
    const cls = classify(SAMPLE_INV);
    const md = renderMarkdown({ inventory: SAMPLE_INV, classification: cls });
    expect(md).toContain("# Threat profile: sample-server v0.1.0");
  });

  it("includes a Summary section with all distributions", () => {
    const cls = classify(SAMPLE_INV);
    const md = renderMarkdown({ inventory: SAMPLE_INV, classification: cls });
    expect(md).toContain("## Summary");
    expect(md).toContain("**Tools:** 3");
    expect(md).toContain("Data-class distribution");
    expect(md).toContain("Authority-level distribution");
    expect(md).toContain("Confused-deputy candidates");
  });

  it("orders tool sections by authority (privileged → destructive → write → read)", () => {
    const cls = classify(SAMPLE_INV);
    const md = renderMarkdown({ inventory: SAMPLE_INV, classification: cls });
    // shell_exec is privileged → first; delete_path destructive → second;
    // read_file read → last.
    const idxShell = md.indexOf("### shell_exec");
    const idxDelete = md.indexOf("### delete_path");
    const idxRead = md.indexOf("### read_file");
    expect(idxShell).toBeGreaterThan(0);
    expect(idxDelete).toBeGreaterThan(idxShell);
    expect(idxRead).toBeGreaterThan(idxDelete);
  });

  it("includes the recommended caveat in a code block per tool", () => {
    const cls = classify(SAMPLE_INV);
    const md = renderMarkdown({ inventory: SAMPLE_INV, classification: cls });
    expect(md).toContain("**Recommended capnagent caveat:**");
    // Each of the 3 tools should have a fenced code block.
    const fenceCount = (md.match(/```\n/g) ?? []).length;
    expect(fenceCount).toBeGreaterThanOrEqual(3);
  });

  it("flags confused-deputy candidates with a warning sigil", () => {
    const cls = classify(SAMPLE_INV);
    const md = renderMarkdown({ inventory: SAMPLE_INV, classification: cls });
    // delete_path and shell_exec are confused-deputy candidates.
    expect(md.match(/⚠️ confused-deputy candidate/g)?.length ?? 0).toBeGreaterThanOrEqual(2);
  });

  it("with fuzz, summarises totals + warns on runtime_error", () => {
    const cls = classify(SAMPLE_INV);
    const fuzz = {
      schema: "mcp-recon/v0.1/fuzz" as const,
      scanned_at: FROZEN_NOW,
      server: SAMPLE_INV.server,
      seed: 0xc0_ffee,
      budget: 10,
      summary: [
        { tool: "read_file", total: 10, ok: 0, protocol_error: 10, runtime_error: 0 },
        { tool: "delete_path", total: 10, ok: 0, protocol_error: 9, runtime_error: 1 },
        { tool: "shell_exec", total: 10, ok: 0, protocol_error: 10, runtime_error: 0 },
      ],
      calls: [],
    };
    const md = renderMarkdown({ inventory: SAMPLE_INV, classification: cls, fuzz });
    expect(md).toContain("**Fuzz:** 30 calls");
    expect(md).toContain("at least one input escaped");
  });

  it("renders without throwing for an empty inventory", () => {
    const empty: ToolInventory = {
      schema: "mcp-recon/v0.1/inventory",
      scanned_at: FROZEN_NOW,
      server: { name: "empty", version: "0" },
      tools: [],
    };
    const cls = classify(empty);
    const md = renderMarkdown({ inventory: empty, classification: cls });
    expect(md).toContain("**Tools:** 0");
  });
});
