/**
 * Server-spec parsing + transport construction.
 *
 * Per docs/SPEC.md, v0.1 supports three forms:
 *
 *   stdio:<command> [args...]      — spawn process; stdio transport
 *   stdio:./path/to/binary --arg   — relative path also accepted
 *   http://localhost:3000          — HTTP transport
 *
 * The HTTP transport stub is here for v0.1; the implementation
 * lands in week 2 once we have a real HTTP-transport MCP server to
 * test against. For week 1 the focus is stdio + the official
 * @modelcontextprotocol/server-filesystem.
 */

import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

/** A parsed server-spec — the union of supported transport variants. */
export type ServerSpec =
	| { kind: "stdio"; command: string; args: string[] }
	| { kind: "http"; url: string };

/**
 * Parse a server-spec string into structured form.
 *
 * Throws if the spec doesn't match a supported shape — we fail loud
 * here rather than silently degrading to a default, because a
 * mistyped spec produces a confusing connect-failure later.
 */
export function parseServerSpec(spec: string): ServerSpec {
	if (spec.startsWith("stdio:")) {
		const rest = spec.slice("stdio:".length).trim();
		if (rest === "") {
			throw new Error(
				`parseServerSpec: stdio: prefix requires a command after the colon (got "${spec}")`,
			);
		}
		// Naive shell-splitting: split on whitespace. Good enough for
		// `stdio:npx @modelcontextprotocol/server-filesystem /tmp` and
		// similar. v0.2 may add proper quoting if real specs need it.
		const parts = rest.split(/\s+/).filter((p) => p.length > 0);
		const command = parts[0];
		if (command === undefined) {
			throw new Error(
				`parseServerSpec: command empty after split (got "${spec}")`,
			);
		}
		return { kind: "stdio", command, args: parts.slice(1) };
	}
	if (spec.startsWith("http://") || spec.startsWith("https://")) {
		return { kind: "http", url: spec };
	}
	throw new Error(
		`parseServerSpec: unrecognised spec "${spec}" — expected stdio:<cmd> or http(s)://...`,
	);
}

/**
 * Open a connected MCP `Client` for the given spec.
 *
 * Caller owns shutdown — pass the result to `closeClient` when
 * finished. We do not auto-close on process exit because some
 * commands (`scan`) hold the client across multiple steps.
 */
export async function openClient(spec: ServerSpec): Promise<Client> {
	const client = new Client(
		{ name: "mcp-recon", version: "0.0.1" },
		{ capabilities: {} },
	);

	switch (spec.kind) {
		case "stdio": {
			const transport = new StdioClientTransport({
				command: spec.command,
				args: spec.args,
			});
			await client.connect(transport);
			return client;
		}
		case "http": {
			// Stub — v0.1 week 2 will wire this against an HTTP MCP
			// server. The MCP SDK's HTTP transport is the natural target.
			throw new Error(
				`openClient: http transport not implemented in v0.1 scaffold (got ${spec.url})`,
			);
		}
	}
}

/** Close a client opened by `openClient`. Safe to call multiple times. */
export async function closeClient(client: Client): Promise<void> {
	await client.close();
}
