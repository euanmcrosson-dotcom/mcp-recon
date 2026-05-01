/**
 * mcp-recon performance benchmarks.
 *
 * Measures the pure-function pipeline: classify, classify+fuzz-fold,
 * planCaveats, and renderMarkdown — for the 4 reference servers
 * (filesystem / everything / memory / sequential-thinking) plus
 * synthetic-scale inventories at 10/100/1000/10000 tools.
 *
 * The benchmarks here intentionally exclude I/O (transport, JSON.parse
 * of fixtures, fs.write of report) — those are bounded by network /
 * disk. The targets in docs/SPEC.md ("Classify + report: O(tools),
 * < 1 second") describe the pure CPU pipeline, which is what this
 * suite measures.
 *
 * Output:
 *   - human-readable text → stdout
 *   - JSON → bench/results/v<git-sha>.json (or `dirty.json` outside git)
 *
 * Run via `npm run bench --workspace=@mcp-recon/cli`.
 */

import { Bench } from "tinybench";
import { execSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  classify,
  renderMarkdown,
  synthesizeCaveat,
  type ClassificationResults,
  type ToolInventory,
  type FuzzResults,
  type EnumeratedTool,
} from "../src/index.js";
import { extractToolFacts } from "../src/fuzz/schema.js";

// ─── locations ──────────────────────────────────────────────────────

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "..", "..", "..");
const PUBLIC_SERVERS = resolve(REPO_ROOT, "examples", "public-servers");
const RESULTS_DIR = resolve(__dirname, "results");

const SERVERS = [
  "server-filesystem",
  "server-everything",
  "server-memory",
  "server-sequential-thinking",
] as const;

// ─── helpers ────────────────────────────────────────────────────────

interface ServerFixture {
  slug: string;
  inventory: ToolInventory;
  fuzz: FuzzResults;
  classification: ClassificationResults;
}

function loadFixture(slug: string): ServerFixture {
  const dir = resolve(PUBLIC_SERVERS, slug);
  const inventory = JSON.parse(
    readFileSync(resolve(dir, "inventory.json"), "utf8"),
  ) as ToolInventory;
  const fuzz = JSON.parse(
    readFileSync(resolve(dir, "fuzz.json"), "utf8"),
  ) as FuzzResults;
  const classification = JSON.parse(
    readFileSync(resolve(dir, "classification.json"), "utf8"),
  ) as ClassificationResults;
  return { slug, inventory, fuzz, classification };
}

/**
 * `planCaveats` — for a classification, materialize a recommended
 * capnagent caveat string per tool (using deployment bindings to
 * substitute placeholders). This is a thin O(tools) loop that calls
 * `synthesizeCaveat` once per tool; equivalent to the per-tool work
 * `classify` already does, called separately to bench downstream
 * caveat-planning hot paths.
 */
export interface CaveatBindings {
  caller: string;
  sandbox: string;
  origin: string;
  expiry: string;
  command: string;
  bounded: string;
}

export function planCaveats(
  classification: ClassificationResults,
  inventory: ToolInventory,
  bindings: CaveatBindings,
): string[] {
  const toolByName = new Map<string, EnumeratedTool>();
  for (const t of inventory.tools) toolByName.set(t.name, t);

  const out: string[] = [];
  for (const c of classification.classifications) {
    const tool = toolByName.get(c.tool);
    if (tool === undefined) continue;
    const facts = extractToolFacts(tool.name, tool.inputSchema);
    let s = synthesizeCaveat({
      tool: c.tool,
      data_class: c.data_class,
      authority_level: c.authority_level,
      facts,
    });
    s = s
      .replaceAll("<your-caller-id>", bindings.caller)
      .replaceAll("<your-sandbox-prefix>", bindings.sandbox)
      .replaceAll("<your-allowed-origin>", bindings.origin)
      .replaceAll("<your-cap-expiry>", bindings.expiry)
      .replaceAll("<your-allowed-command-prefix>", bindings.command)
      .replaceAll("<your-bounded-prefix>", bindings.bounded);
    out.push(s);
  }
  return out;
}

const BINDINGS: CaveatBindings = {
  caller: "agent-001",
  sandbox: "/srv/sandbox",
  origin: "https://example.com",
  expiry: "2026-12-31T23:59:59Z",
  command: "/usr/bin/ls",
  bounded: "/srv/bounded",
};

// ─── synthetic generator ───────────────────────────────────────────

/**
 * Generate a synthetic inventory of `n` tools with a representative
 * mix of name/description shapes (read/write/exec/network) so that
 * `classify` exercises real rule paths, not a degenerate fast path.
 */
function syntheticInventory(n: number): ToolInventory {
  const ARCHETYPES = [
    {
      name: "read_file",
      description: "Read the complete contents of a file as text.",
      schema: {
        type: "object" as const,
        properties: {
          path: { type: "string" as const },
          tail: { type: "number" as const },
        },
        required: ["path"],
      },
    },
    {
      name: "write_file",
      description: "Write content to a file at the given path. Overwrites.",
      schema: {
        type: "object" as const,
        properties: {
          path: { type: "string" as const },
          content: { type: "string" as const },
        },
        required: ["path", "content"],
      },
    },
    {
      name: "delete_entry",
      description: "Permanently delete a file or directory at the given path.",
      schema: {
        type: "object" as const,
        properties: {
          path: { type: "string" as const },
        },
        required: ["path"],
      },
    },
    {
      name: "execute_shell",
      description: "Execute an arbitrary shell command and return stdout.",
      schema: {
        type: "object" as const,
        properties: {
          command: { type: "string" as const },
        },
        required: ["command"],
      },
    },
    {
      name: "fetch_url",
      description: "Make an HTTP request to the given URL and return the body.",
      schema: {
        type: "object" as const,
        properties: {
          url: { type: "string" as const },
          method: { type: "string" as const, enum: ["GET", "POST"] },
        },
        required: ["url"],
      },
    },
    {
      name: "send_email",
      description: "Send an email message to the given recipient.",
      schema: {
        type: "object" as const,
        properties: {
          to: { type: "string" as const },
          subject: { type: "string" as const },
          body: { type: "string" as const },
        },
        required: ["to", "body"],
      },
    },
    {
      name: "charge_payment",
      description: "Charge a payment to the given account in the given amount.",
      schema: {
        type: "object" as const,
        properties: {
          account: { type: "string" as const },
          amount_cents: { type: "integer" as const },
        },
        required: ["account", "amount_cents"],
      },
    },
    {
      name: "get_system_info",
      description: "Return information about the host operating system.",
      schema: {
        type: "object" as const,
        properties: {},
      },
    },
  ];

  const tools: EnumeratedTool[] = [];
  for (let i = 0; i < n; i++) {
    const a = ARCHETYPES[i % ARCHETYPES.length]!;
    tools.push({
      name: `${a.name}_${i}`,
      description: a.description,
      inputSchema: a.schema,
    });
  }

  return {
    schema: "mcp-recon/v0.1/inventory" as const,
    scanned_at: new Date().toISOString(),
    server: { name: `synthetic-${n}`, version: "0.0.0" },
    tools,
  };
}

// ─── bench result types ────────────────────────────────────────────

interface BenchRow {
  group: string;
  task: string;
  meanMs: number;
  medianMs: number;
  p95Ms: number;
  opsPerSec: number;
  samples: number;
}

function rowFromBench(group: string, task: import("tinybench").Task): BenchRow {
  // tinybench reports times in *milliseconds*. `samples` is the array of
  // per-iteration durations (also ms). We compute median/p95 ourselves
  // since the public API on the version in use isn't guaranteed to expose
  // them on every release.
  const r = task.result!;
  const samples = (r.samples ?? []).slice().sort((a, b) => a - b);
  const median =
    samples.length === 0
      ? r.mean
      : samples[Math.floor(samples.length / 2)] ?? r.mean;
  const p95 =
    samples.length === 0
      ? r.mean
      : samples[Math.min(samples.length - 1, Math.floor(samples.length * 0.95))] ?? r.mean;
  return {
    group,
    task: task.name,
    meanMs: r.mean,
    medianMs: median,
    p95Ms: p95,
    opsPerSec: r.hz,
    samples: r.samples?.length ?? 0,
  };
}

// ─── main ──────────────────────────────────────────────────────────

async function main(): Promise<void> {
  const startedAt = Date.now();
  console.log("mcp-recon benchmark suite");
  console.log("=========================");
  console.log(`node:        ${process.version}`);
  console.log(`platform:    ${process.platform} ${process.arch}`);
  console.log(`time:        ${new Date().toISOString()}`);
  console.log("");

  const fixtures = SERVERS.map(loadFixture);

  // Pure-function bench: classify (no fuzz)
  const benchClassify = new Bench({ time: 250, iterations: 100, warmupIterations: 10 });
  for (const f of fixtures) {
    benchClassify.add(`classify(${f.slug})`, () => {
      classify(f.inventory);
    });
  }
  await benchClassify.warmup();
  await benchClassify.run();

  // classify with fuzz folded in
  const benchClassifyFuzz = new Bench({ time: 250, iterations: 100, warmupIterations: 10 });
  for (const f of fixtures) {
    benchClassifyFuzz.add(`classify+fuzz(${f.slug})`, () => {
      classify(f.inventory, f.fuzz);
    });
  }
  await benchClassifyFuzz.warmup();
  await benchClassifyFuzz.run();

  // renderMarkdown
  const benchRender = new Bench({ time: 250, iterations: 100, warmupIterations: 10 });
  for (const f of fixtures) {
    benchRender.add(`renderMarkdown(${f.slug})`, () => {
      renderMarkdown({ inventory: f.inventory, classification: f.classification });
    });
  }
  await benchRender.warmup();
  await benchRender.run();

  // planCaveats
  const benchPlan = new Bench({ time: 250, iterations: 100, warmupIterations: 10 });
  for (const f of fixtures) {
    benchPlan.add(`planCaveats(${f.slug})`, () => {
      planCaveats(f.classification, f.inventory, BINDINGS);
    });
  }
  await benchPlan.warmup();
  await benchPlan.run();

  // synthetic-scale: classify + planCaveats at 10/100/1000/10000
  const SCALES = [10, 100, 1000, 10000];
  const synthetic = SCALES.map((n) => ({
    n,
    inventory: syntheticInventory(n),
  }));
  // pre-compute classifications so planCaveats has something to walk
  const syntheticClassified = synthetic.map((s) => ({
    n: s.n,
    inventory: s.inventory,
    classification: classify(s.inventory),
  }));

  // For larger sizes, fewer iterations is fine (one call already takes
  // longer). tinybench will adapt with the time budget.
  const benchScaleClassify = new Bench({ time: 500, iterations: 30, warmupIterations: 3 });
  for (const s of synthetic) {
    benchScaleClassify.add(`classify(synthetic-n=${s.n})`, () => {
      classify(s.inventory);
    });
  }
  await benchScaleClassify.warmup();
  await benchScaleClassify.run();

  const benchScalePlan = new Bench({ time: 500, iterations: 30, warmupIterations: 3 });
  for (const s of syntheticClassified) {
    benchScalePlan.add(`planCaveats(synthetic-n=${s.n})`, () => {
      planCaveats(s.classification, s.inventory, BINDINGS);
    });
  }
  await benchScalePlan.warmup();
  await benchScalePlan.run();

  // ─── collect ────────────────────────────────────────────────────
  const rows: BenchRow[] = [];
  for (const t of benchClassify.tasks) rows.push(rowFromBench("classify", t));
  for (const t of benchClassifyFuzz.tasks) rows.push(rowFromBench("classify+fuzz", t));
  for (const t of benchRender.tasks) rows.push(rowFromBench("renderMarkdown", t));
  for (const t of benchPlan.tasks) rows.push(rowFromBench("planCaveats", t));
  for (const t of benchScaleClassify.tasks) rows.push(rowFromBench("scale-classify", t));
  for (const t of benchScalePlan.tasks) rows.push(rowFromBench("scale-planCaveats", t));

  // ─── print ──────────────────────────────────────────────────────
  printTable(rows);

  const wallSecs = (Date.now() - startedAt) / 1000;
  console.log("");
  console.log(`total wall-clock: ${wallSecs.toFixed(1)}s`);

  // ─── persist ───────────────────────────────────────────────────
  const sha = gitSha();
  mkdirSync(RESULTS_DIR, { recursive: true });
  const out = {
    schema: "mcp-recon/v0.1/bench-results",
    git_sha: sha,
    node: process.version,
    platform: `${process.platform}/${process.arch}`,
    started_at: new Date(startedAt).toISOString(),
    wall_clock_seconds: wallSecs,
    rows,
  };
  const outPath = resolve(RESULTS_DIR, `v${sha}.json`);
  writeFileSync(outPath, JSON.stringify(out, null, 2) + "\n", "utf8");
  console.log(`results written: ${outPath}`);

  // also overwrite "latest.json" for easy diffing
  const latestPath = resolve(RESULTS_DIR, "latest.json");
  writeFileSync(latestPath, JSON.stringify(out, null, 2) + "\n", "utf8");
  console.log(`results written: ${latestPath}`);
}

function printTable(rows: BenchRow[]): void {
  const header = ["group", "task", "mean (ms)", "median (ms)", "p95 (ms)", "ops/sec", "n"];
  const lines: string[][] = [header];
  for (const r of rows) {
    lines.push([
      r.group,
      r.task,
      fmt(r.meanMs),
      fmt(r.medianMs),
      fmt(r.p95Ms),
      r.opsPerSec.toFixed(0),
      r.samples.toString(),
    ]);
  }
  const widths = header.map((_, i) => Math.max(...lines.map((row) => row[i]!.length)));
  for (const row of lines) {
    console.log(row.map((cell, i) => cell.padEnd(widths[i]!)).join("  "));
  }
}

function fmt(ms: number): string {
  if (ms < 0.01) return ms.toFixed(5);
  if (ms < 1) return ms.toFixed(4);
  if (ms < 10) return ms.toFixed(3);
  return ms.toFixed(2);
}

function gitSha(): string {
  try {
    return execSync("git rev-parse --short HEAD", { cwd: REPO_ROOT })
      .toString()
      .trim();
  } catch {
    return "dirty";
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
