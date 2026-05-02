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
 * Options for `enumerate`.
 *
 * `timeoutMs` is forwarded to the SDK's `client.listTools` request
 * options. Defaults to 30s — tighter than the SDK's 60s default,
 * because mcp-recon scans untrusted servers and a hostile server
 * that never replies should not hang the operator. Pass a smaller
 * value when scripting against many servers; pass a larger value
 * when scanning a slow but trusted server with a huge tool list.
 *
 * `maxDescriptionChars` bounds individual tool descriptions; longer
 * values are truncated (with an `…[truncated by mcp-recon]` marker
 * appended) before being stored in the inventory. Real-world tools
 * have descriptions under 2KB; the 64KB default leaves headroom
 * while bounding memory use of a hostile server returning a 10MB
 * description in every tool. Pass `Infinity` to disable.
 *
 * Both options are discovered via `adversarial-servers/` fixtures
 * (see `__tests__/adversarial.integration.test.ts`).
 */
export interface EnumerateOptions {
  /** Max time, in ms, to wait for `tools/list`. Default: 30000. */
  timeoutMs?: number;
  /** Per-tool description cap, in chars. Default: 65536. */
  maxDescriptionChars?: number;
}

/** Default `tools/list` timeout. See `EnumerateOptions.timeoutMs`. */
export const DEFAULT_ENUMERATE_TIMEOUT_MS = 30_000;

/** Default per-tool description cap. See `EnumerateOptions.maxDescriptionChars`. */
export const DEFAULT_MAX_DESCRIPTION_CHARS = 64 * 1024;

/** Marker appended to truncated descriptions so reviewers see the cap fired. */
export const TRUNCATION_MARKER = "…[truncated by mcp-recon]";

function clampDescription(s: string | undefined, cap: number): string | undefined {
  if (s === undefined) return undefined;
  if (s.length <= cap) return s;
  // Reserve room for the marker so the final string is at most `cap`.
  const room = Math.max(0, cap - TRUNCATION_MARKER.length);
  return s.slice(0, room) + TRUNCATION_MARKER;
}

/**
 * Enumerate all tools exposed by the connected client.
 *
 * v0.1 is a thin shim over `client.listTools()` plus a self-
 * describing wrapper. We do not classify here — that's the
 * `classify` command's job. We do not fuzz here — that's `fuzz`.
 * Single responsibility.
 *
 * Enforces a per-call timeout (`opts.timeoutMs`, default 30s) so a
 * hostile server that accepts the handshake but never answers
 * `tools/list` cannot hang the operator forever.
 */
export async function enumerate(
  client: Client,
  opts: EnumerateOptions = {},
): Promise<ToolInventory> {
  const timeoutMs = opts.timeoutMs ?? DEFAULT_ENUMERATE_TIMEOUT_MS;
  const maxDescriptionChars = opts.maxDescriptionChars ?? DEFAULT_MAX_DESCRIPTION_CHARS;
  const result = await client.listTools(undefined, { timeout: timeoutMs });
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
      description: clampDescription(t.description, maxDescriptionChars),
      inputSchema: t.inputSchema,
    })),
  };
}
