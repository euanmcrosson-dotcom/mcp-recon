/**
 * Unit tests for the `scan` orchestrator using a mock MCP client.
 *
 * The real-server end-to-end is exercised by the LIVE_MCP integration
 * tests (enumerate.integration.test.ts + fuzz.integration.test.ts). This
 * test stubs the SDK Client so we can exercise the full pipeline without
 * spawning a subprocess.
 */

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { Client } from "@modelcontextprotocol/sdk/client/index.js";

import { scan } from "../scan/index.js";

/** Minimal MCP-shaped fake. Only the methods scan() actually calls. */
function makeFakeClient(): Client {
  const fake = {
    listTools: async () => ({
      tools: [
        {
          name: "read_file",
          description: "Read a file at path.",
          inputSchema: {
            type: "object",
            properties: { path: { type: "string" } },
            required: ["path"],
          },
        },
        {
          name: "delete_path",
          description: "Delete the file at path.",
          inputSchema: {
            type: "object",
            properties: { path: { type: "string" } },
            required: ["path"],
          },
        },
      ],
    }),
    callTool: async () => ({
      // Return as protocol_error-ish content for any args; the
      // shape mirrors what a real MCP server returns when refusing.
      isError: true,
      content: [{ type: "text", text: "denied" }],
    }),
    getServerVersion: () => ({ name: "fake-server", version: "0.0.0" }),
    close: async () => {},
  } as unknown as Client;
  return fake;
}

let outDir: string;

beforeEach(() => {
  outDir = fs.mkdtempSync(path.join(os.tmpdir(), "mcp-recon-scan-"));
});

afterEach(() => {
  fs.rmSync(outDir, { recursive: true, force: true });
});

describe("scan orchestrator", () => {
  it("returns all four bundled results", async () => {
    const client = makeFakeClient();
    const result = await scan(client, { budget: 3, timeoutMs: 1000 });
    expect(result.inventory.tools.length).toBe(2);
    expect(result.fuzz.calls.length).toBeGreaterThan(0);
    expect(result.classification.classifications.length).toBe(2);
    expect(result.reportMarkdown).toContain("Threat profile: fake-server");
  });

  it("writes 4 artefacts to outDir when set", async () => {
    const client = makeFakeClient();
    await scan(client, { budget: 3, timeoutMs: 1000, outDir });
    expect(fs.existsSync(path.join(outDir, "inventory.json"))).toBe(true);
    expect(fs.existsSync(path.join(outDir, "fuzz.json"))).toBe(true);
    expect(fs.existsSync(path.join(outDir, "classification.json"))).toBe(true);
    expect(fs.existsSync(path.join(outDir, "report.md"))).toBe(true);

    // Schema tags survive the round-trip.
    const inv = JSON.parse(fs.readFileSync(path.join(outDir, "inventory.json"), "utf8"));
    const fz = JSON.parse(fs.readFileSync(path.join(outDir, "fuzz.json"), "utf8"));
    const cls = JSON.parse(fs.readFileSync(path.join(outDir, "classification.json"), "utf8"));
    expect(inv.schema).toBe("mcp-recon/v0.1/inventory");
    expect(fz.schema).toBe("mcp-recon/v0.1/fuzz");
    expect(cls.schema).toBe("mcp-recon/v0.1/classification");
  });

  it("classification reflects fuzz informed bonus when fuzz has ok results", async () => {
    // Build a client where every callTool returns a non-error (so
    // every fuzz attempt registers as `ok`).
    const okClient = {
      listTools: async () => ({
        tools: [
          {
            name: "read_file",
            description: "Read a file.",
            inputSchema: {
              type: "object",
              properties: { path: { type: "string" } },
              required: ["path"],
            },
          },
        ],
      }),
      callTool: async () => ({ isError: false, content: [{ type: "text", text: "ok" }] }),
      getServerVersion: () => ({ name: "ok-server", version: "0.0.0" }),
      close: async () => {},
    } as unknown as Client;

    const result = await scan(okClient, { budget: 3, timeoutMs: 1000 });
    expect(result.classification.fuzz_informed).toBe(true);
  });

  it("does not write to disk when outDir is omitted", async () => {
    const before = fs.readdirSync(outDir);
    const client = makeFakeClient();
    await scan(client, { budget: 2, timeoutMs: 1000 });
    const after = fs.readdirSync(outDir);
    expect(after).toEqual(before);
  });

  it("produces a report.md with at least one fenced code block (a recommended caveat)", async () => {
    const client = makeFakeClient();
    await scan(client, { budget: 2, timeoutMs: 1000, outDir });
    const md = fs.readFileSync(path.join(outDir, "report.md"), "utf8");
    expect(md).toMatch(/```/);
    expect(md).toContain("**Recommended capnagent caveat:**");
  });
});
