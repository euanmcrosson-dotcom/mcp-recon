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

import * as fs from "node:fs";

import {
  CLASSIFICATION_SCHEMA,
  classify,
  closeClient,
  enumerate,
  fuzz,
  openClient,
  parseServerSpec,
  renderMarkdown,
  scan,
} from "../index.js";

import type { ToolInventory } from "../enumerate.js";
import type { FuzzResults } from "../fuzz/types.js";
import type { ClassificationResults } from "../classify/types.js";

function usage(): never {
  process.stderr.write(
    [
      "mcp-recon — reverse-engineer MCP server tool surfaces",
      "",
      "Usage:",
      "  mcp-recon enumerate <server-spec>",
      "  mcp-recon fuzz <server-spec> [--budget=N] [--seed=N]",
      "  mcp-recon classify <inventory.json> [--fuzz=<fuzz.json>]",
      "  mcp-recon report <inventory.json> <classification.json> [--fuzz=<fuzz.json>]",
      "  mcp-recon scan <server-spec> --out=<dir> [--budget=N] [--seed=N]",
      "",
      "Server-spec forms:",
      "  stdio:<command> [args...]    — spawn process, talk over stdio",
      "  http://host:port             — HTTP transport (v0.1 week 2)",
      "",
      "Examples:",
      "  mcp-recon enumerate stdio:npx -y @modelcontextprotocol/server-filesystem /tmp",
      "  mcp-recon fuzz stdio:npx -y @modelcontextprotocol/server-filesystem /tmp --budget=20",
      "  mcp-recon classify inv.json --fuzz=fz.json > class.json",
      "  mcp-recon report inv.json class.json --fuzz=fz.json > report.md",
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
      return await runFuzz(argv.slice(1));
    case "classify":
      return runClassify(argv.slice(1));
    case "report":
      return runReport(argv.slice(1));
    case "scan":
      return await runScan(argv.slice(1));
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

async function runFuzz(args: string[]): Promise<number> {
  // Pull --budget=N and --seed=N flags out; the first positional is the spec.
  let budget: number | undefined;
  let seed: number | undefined;
  let spec: string | undefined;
  for (const arg of args) {
    if (arg.startsWith("--budget=")) {
      const n = Number.parseInt(arg.slice("--budget=".length), 10);
      if (Number.isNaN(n) || n <= 0) {
        process.stderr.write(`mcp-recon fuzz: invalid --budget value\n`);
        return 2;
      }
      budget = n;
    } else if (arg.startsWith("--seed=")) {
      const n = Number.parseInt(arg.slice("--seed=".length), 10);
      if (Number.isNaN(n)) {
        process.stderr.write(`mcp-recon fuzz: invalid --seed value\n`);
        return 2;
      }
      seed = n;
    } else if (!spec) {
      spec = arg;
    } else {
      process.stderr.write(`mcp-recon fuzz: unexpected argument ${arg}\n`);
      return 2;
    }
  }
  if (!spec) {
    process.stderr.write("mcp-recon fuzz: missing <server-spec>\n");
    return 2;
  }

  const parsed = parseServerSpec(spec);
  process.stderr.write(`mcp-recon: connecting to ${spec}...\n`);

  const client = await openClient(parsed);
  try {
    const inventory = await enumerate(client);
    process.stderr.write(
      `mcp-recon: fuzzing ${inventory.tools.length} tools (budget=${budget ?? 200}, seed=${seed ?? "default"})...\n`,
    );
    const opts: Parameters<typeof fuzz>[2] = {};
    if (budget !== undefined) opts.budget = budget;
    if (seed !== undefined) opts.seed = seed;
    const results = await fuzz(client, inventory, opts);

    const totalCalls = results.calls.length;
    const totalOk = results.summary.reduce((acc, s) => acc + s.ok, 0);
    const totalProto = results.summary.reduce((acc, s) => acc + s.protocol_error, 0);
    const totalRuntime = results.summary.reduce((acc, s) => acc + s.runtime_error, 0);
    process.stderr.write(
      `mcp-recon: ${totalCalls} calls — ok=${totalOk} protocol_error=${totalProto} runtime_error=${totalRuntime}\n`,
    );

    process.stdout.write(`${JSON.stringify(results, null, 2)}\n`);
    return 0;
  } finally {
    await closeClient(client);
  }
}

function runClassify(args: string[]): number {
  let inventoryPath: string | undefined;
  let fuzzPath: string | undefined;
  for (const arg of args) {
    if (arg.startsWith("--fuzz=")) {
      fuzzPath = arg.slice("--fuzz=".length);
    } else if (!inventoryPath) {
      inventoryPath = arg;
    } else {
      process.stderr.write(`mcp-recon classify: unexpected argument ${arg}\n`);
      return 2;
    }
  }
  if (!inventoryPath) {
    process.stderr.write("mcp-recon classify: missing <inventory.json>\n");
    return 2;
  }

  const inventory = readJson<ToolInventory>(inventoryPath);
  const fuzzData = fuzzPath ? readJson<FuzzResults>(fuzzPath) : undefined;
  const result = classify(inventory, fuzzData);

  process.stderr.write(
    `mcp-recon: classified ${result.classifications.length} tools (fuzz_informed=${result.fuzz_informed})\n`,
  );
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  return 0;
}

function runReport(args: string[]): number {
  let inventoryPath: string | undefined;
  let classificationPath: string | undefined;
  let fuzzPath: string | undefined;
  for (const arg of args) {
    if (arg.startsWith("--fuzz=")) {
      fuzzPath = arg.slice("--fuzz=".length);
    } else if (!inventoryPath) {
      inventoryPath = arg;
    } else if (!classificationPath) {
      classificationPath = arg;
    } else {
      process.stderr.write(`mcp-recon report: unexpected argument ${arg}\n`);
      return 2;
    }
  }
  if (!inventoryPath || !classificationPath) {
    process.stderr.write(
      "mcp-recon report: missing arguments — needs <inventory.json> <classification.json>\n",
    );
    return 2;
  }

  const inventory = readJson<ToolInventory>(inventoryPath);
  const classification = readJson<ClassificationResults>(classificationPath);
  if (classification.schema !== CLASSIFICATION_SCHEMA) {
    process.stderr.write(
      `mcp-recon report: classification.json schema is "${classification.schema}", expected "${CLASSIFICATION_SCHEMA}"\n`,
    );
    return 64;
  }
  const fuzzData = fuzzPath ? readJson<FuzzResults>(fuzzPath) : undefined;
  const md = renderMarkdown(
    fuzzData ? { inventory, classification, fuzz: fuzzData } : { inventory, classification },
  );
  process.stdout.write(md);
  if (!md.endsWith("\n")) process.stdout.write("\n");
  return 0;
}

function readJson<T>(filePath: string): T {
  const text = fs.readFileSync(filePath, "utf8");
  return JSON.parse(text) as T;
}

async function runScan(args: string[]): Promise<number> {
  let spec: string | undefined;
  let outDir: string | undefined;
  let budget: number | undefined;
  let seed: number | undefined;
  for (const arg of args) {
    if (arg.startsWith("--out=")) {
      outDir = arg.slice("--out=".length);
    } else if (arg.startsWith("--budget=")) {
      const n = Number.parseInt(arg.slice("--budget=".length), 10);
      if (Number.isNaN(n) || n <= 0) {
        process.stderr.write("mcp-recon scan: invalid --budget value\n");
        return 2;
      }
      budget = n;
    } else if (arg.startsWith("--seed=")) {
      const n = Number.parseInt(arg.slice("--seed=".length), 10);
      if (Number.isNaN(n)) {
        process.stderr.write("mcp-recon scan: invalid --seed value\n");
        return 2;
      }
      seed = n;
    } else if (!spec) {
      spec = arg;
    } else {
      process.stderr.write(`mcp-recon scan: unexpected argument ${arg}\n`);
      return 2;
    }
  }

  if (!spec) {
    process.stderr.write("mcp-recon scan: missing <server-spec>\n");
    return 2;
  }
  if (!outDir) {
    process.stderr.write("mcp-recon scan: --out=<dir> is required\n");
    return 2;
  }

  const parsed = parseServerSpec(spec);
  process.stderr.write(`mcp-recon: connecting to ${spec}...\n`);

  const client = await openClient(parsed);
  try {
    const opts: Parameters<typeof scan>[1] = { outDir };
    if (budget !== undefined) opts.budget = budget;
    if (seed !== undefined) opts.seed = seed;
    process.stderr.write(`mcp-recon: running enumerate → fuzz → classify → report...\n`);
    const result = await scan(client, opts);

    const totalOk = result.fuzz.summary.reduce((acc, s) => acc + s.ok, 0);
    const totalProto = result.fuzz.summary.reduce((acc, s) => acc + s.protocol_error, 0);
    const totalRuntime = result.fuzz.summary.reduce((acc, s) => acc + s.runtime_error, 0);
    const cdpCount = result.classification.classifications.filter(
      (c) => c.confused_deputy_candidate,
    ).length;

    process.stderr.write(
      `mcp-recon: ${result.inventory.tools.length} tools, ${cdpCount} confused-deputy candidates\n`,
    );
    process.stderr.write(
      `mcp-recon: fuzz — ok=${totalOk} protocol_error=${totalProto} runtime_error=${totalRuntime}\n`,
    );
    process.stderr.write(`mcp-recon: wrote 4 artefacts to ${outDir}/\n`);
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
