#!/usr/bin/env node
/**
 * `mcp-recon` CLI entry point.
 *
 * v0.1 surface (see docs/SPEC.md §"What v0.1 does"):
 *
 *   mcp-recon enumerate <server-spec>      [implemented in scaffold]
 *   mcp-recon fuzz <server-spec>           [stub — v0.1 week 2]
 *   mcp-recon classify <inventory.json>    [stub — v0.1 week 3]
 *   mcp-recon report <i.json> <f.json> <c.json>  [stub — v0.1 week 3]
 *   mcp-recon scan <server-spec>           [stub — v0.1 week 4]
 *
 * Output is JSON to stdout for machine-parseable commands; logs to
 * stderr so a piped invocation (`mcp-recon enumerate ... | jq ...`)
 * works without contamination.
 */

import { closeClient, enumerate, openClient, parseServerSpec } from "../index.js";

function usage(): never {
  process.stderr.write(
    [
      "mcp-recon — reverse-engineer MCP server tool surfaces",
      "",
      "Usage:",
      "  mcp-recon enumerate <server-spec>",
      "  mcp-recon fuzz <server-spec> [--budget=N]      (v0.1 week 2)",
      "  mcp-recon classify <inventory.json>            (v0.1 week 3)",
      "  mcp-recon report <inventory.json> <fuzz.json> <classification.json>",
      "  mcp-recon scan <server-spec>                   (v0.1 week 4)",
      "",
      "Server-spec forms:",
      "  stdio:<command> [args...]    — spawn process, talk over stdio",
      "  http://host:port             — HTTP transport (v0.1 week 2)",
      "",
      "Examples:",
      "  mcp-recon enumerate stdio:npx @modelcontextprotocol/server-filesystem /tmp",
      "  mcp-recon enumerate stdio:./my-mcp-server",
      "",
    ].join("\n"),
  );
  process.exit(2);
}

async function main(): Promise<number> {
  const argv = process.argv.slice(2);
  const cmd = argv[0];

  if (!cmd || cmd === "--help" || cmd === "-h") {
    usage();
  }

  switch (cmd) {
    case "enumerate":
      return await runEnumerate(argv.slice(1));
    case "fuzz":
    case "classify":
    case "report":
    case "scan":
      process.stderr.write(
        `mcp-recon: '${cmd}' is not implemented in the v0.1 scaffold yet.\n` +
          `See docs/SPEC.md for the milestone schedule.\n`,
      );
      return 64;
    default:
      process.stderr.write(`mcp-recon: unknown command '${cmd}'.\n`);
      usage();
  }
}

async function runEnumerate(args: string[]): Promise<number> {
  const spec = args[0];
  if (!spec) {
    process.stderr.write("mcp-recon enumerate: missing <server-spec>\n");
    return 2;
  }
  const parsed = parseServerSpec(spec);
  process.stderr.write(`mcp-recon: connecting to ${spec}...\n`);

  const client = await openClient(parsed);
  try {
    const inventory = await enumerate(client);
    process.stderr.write(
      `mcp-recon: enumerated ${inventory.tools.length} tools from ${inventory.server.name ?? "unknown server"}\n`,
    );
    process.stdout.write(`${JSON.stringify(inventory, null, 2)}\n`);
    return 0;
  } finally {
    await closeClient(client);
  }
}

main()
  .then((code) => process.exit(code))
  .catch((err) => {
    process.stderr.write(`mcp-recon: ${err instanceof Error ? err.message : String(err)}\n`);
    process.exit(1);
  });
