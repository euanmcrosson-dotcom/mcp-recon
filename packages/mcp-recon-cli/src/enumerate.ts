/**
 * `enumerate` — produce a v0.1 tool inventory document.
 *
 * Connects to the server (caller's responsibility — pass an open
 * Client) and lists every tool. Output shape is documented in
 * docs/SPEC.md §"Output format". The shape is the load-bearing
 * contract that downstream tools (capnagent's caveat-suggestion
 * bridge, the Markdown reporter) consume.
 */

import type { Client } from "@modelcontextprotocol/sdk/client/index.js";

/** Schema-version tag for inventory documents. v0.1 = mcp-recon/v0.1/inventory. */
export const INVENTORY_SCHEMA = "mcp-recon/v0.1/inventory" as const;

/** One tool as it appears in the inventory. */
export interface EnumeratedTool {
  /** Tool name as the server reported it. */
  name: string;
  /** Optional human-readable description from the server. */
  description?: string;
  /** JSON Schema for the tool's input arguments — opaque to v0.1 enumerate. */
  inputSchema: unknown;
}

/** A complete inventory document. */
export interface ToolInventory {
  schema: typeof INVENTORY_SCHEMA;
  scanned_at: string;
  server: {
    /** Whatever name the server self-identified as via initialize. */
    name?: string;
    /** Whatever version the server self-identified as. */
    version?: string;
  };
  tools: EnumeratedTool[];
}

/**
 * Enumerate all tools exposed by the connected client.
 *
 * v0.1 is a thin shim over `client.listTools()` plus a self-
 * describing wrapper. We do not classify here — that's the
 * `classify` command's job. We do not fuzz here — that's `fuzz`.
 * Single responsibility.
 */
export async function enumerate(client: Client): Promise<ToolInventory> {
  const result = await client.listTools();
  // The SDK exposes the server's identity via getServerVersion(); in
  // some SDK versions this is an awaitable getter and in others a
  // direct property. Probe defensively so we don't break across
  // minor version bumps.
  const serverInfo = (client as unknown as {
    getServerVersion?: () => { name?: string; version?: string } | undefined;
  }).getServerVersion?.();

  return {
    schema: INVENTORY_SCHEMA,
    scanned_at: new Date().toISOString(),
    server: {
      name: serverInfo?.name,
      version: serverInfo?.version,
    },
    tools: result.tools.map((t) => ({
      name: t.name,
      description: t.description,
      inputSchema: t.inputSchema,
    })),
  };
}
