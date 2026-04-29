/**
 * Live integration test against the official `@modelcontextprotocol/
 * server-filesystem`. Spawns the server as a real subprocess and runs
 * the full `enumerate` round-trip.
 *
 * Gated on `LIVE_MCP=1` so default `npm test` stays hermetic. The
 * vitest config in `vitest.config.ts` excludes this file unless the
 * env var is set. Run with:
 *
 *     LIVE_MCP=1 npm test -w @mcp-recon/cli
 *
 * What this asserts (the contract):
 *
 *   1. `parseServerSpec` + `openClient` against a stdio command
 *      successfully connect.
 *   2. `enumerate` returns a v0.1 inventory document with the right
 *      schema tag.
 *   3. The filesystem server reports ≥ 10 tools — pinned because the
 *      official server has 14 as of v0.2.0; this asserts the harness
 *      doesn't silently truncate or skip.
 *   4. `closeClient` returns cleanly without leaking the subprocess.
 */

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  INVENTORY_SCHEMA,
  closeClient,
  enumerate,
  openClient,
  parseServerSpec,
} from "../index.js";

import type { Client } from "@modelcontextprotocol/sdk/client/index.js";

let sandbox: string;
let client: Client;

beforeAll(async () => {
  sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "mcp-recon-it-"));
  const spec = parseServerSpec(
    `stdio:npx -y @modelcontextprotocol/server-filesystem ${sandbox}`,
  );
  client = await openClient(spec);
}, 60_000);

afterAll(async () => {
  if (client) await closeClient(client);
  if (sandbox) fs.rmSync(sandbox, { recursive: true, force: true });
});

describe("enumerate against @modelcontextprotocol/server-filesystem (LIVE_MCP)", () => {
  it("emits a valid v0.1 inventory document", async () => {
    const inv = await enumerate(client);

    expect(inv.schema).toBe(INVENTORY_SCHEMA);
    expect(inv.scanned_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    expect(inv.server.name).toBeTruthy();
    expect(inv.server.version).toBeTruthy();
  });

  it("reports a non-trivial tool count (>= 10) — guards against silent truncation", async () => {
    const inv = await enumerate(client);
    expect(inv.tools.length).toBeGreaterThanOrEqual(10);
  });

  it("every tool has a name and an inputSchema", async () => {
    const inv = await enumerate(client);

    for (const tool of inv.tools) {
      expect(tool.name).toBeTruthy();
      expect(typeof tool.name).toBe("string");
      expect(tool.inputSchema).toBeDefined();
      // Most filesystem tools take object-shaped args; pin the
      // structural expectation as a guard rather than a tight
      // assertion so a future server version with different shape
      // doesn't break the test for no security reason.
      expect(typeof tool.inputSchema).toBe("object");
    }
  });

  it("includes the load-bearing read + write tool families", async () => {
    const inv = await enumerate(client);
    const names = new Set(inv.tools.map((t) => t.name));

    // Read-side: at least one of these must be present.
    const reads = ["read_file", "read_text_file", "list_directory", "directory_tree"];
    expect(reads.some((n) => names.has(n))).toBe(true);

    // Write-side: at least one of these must be present. The whole
    // point of capnagent's mcp-fs-agent example is to bound these,
    // so they MUST be visible to mcp-recon for the threat profile
    // to be honest.
    const writes = ["write_file", "edit_file", "create_directory", "move_file"];
    expect(writes.some((n) => names.has(n))).toBe(true);
  });

  it("re-running enumerate produces the same tool set (deterministic shape)", async () => {
    const a = await enumerate(client);
    const b = await enumerate(client);

    const namesA = a.tools.map((t) => t.name).sort();
    const namesB = b.tools.map((t) => t.name).sort();
    expect(namesA).toEqual(namesB);

    // scanned_at differs between runs (it's a timestamp), but the
    // tool inventory itself should be identical.
    expect(a.tools.length).toBe(b.tools.length);
  });
});
