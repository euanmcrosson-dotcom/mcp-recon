/**
 * `scan` — one-shot orchestrator: enumerate → fuzz → classify → report.
 *
 * The daily-driver command. Composes the four primitives defined in
 * `enumerate.ts`, `fuzz/index.ts`, `classify/index.ts`, `report/index.ts`
 * and writes all four artefacts to a single output directory.
 *
 * Returns the bag of intermediate values so a caller (CLI or library)
 * can post-process beyond the on-disk artefacts.
 */

import * as fs from "node:fs";
import * as path from "node:path";

import type { Client } from "@modelcontextprotocol/sdk/client/index.js";

import { classify } from "../classify/index.js";
import type { ClassificationResults } from "../classify/types.js";
import { enumerate } from "../enumerate.js";
import type { ToolInventory } from "../enumerate.js";
import { fuzz } from "../fuzz/index.js";
import type { FuzzOptions, FuzzResults } from "../fuzz/types.js";
import { renderMarkdown } from "../report/index.js";

export interface ScanResult {
  inventory: ToolInventory;
  fuzz: FuzzResults;
  classification: ClassificationResults;
  reportMarkdown: string;
}

export interface ScanOptions extends FuzzOptions {
  /** If set, write the four artefacts to this directory. */
  outDir?: string;
}

/**
 * Run the full pipeline. Caller owns the client (open + close).
 *
 * Side effects (only when `outDir` is provided):
 *   - mkdir -p outDir
 *   - write inventory.json / fuzz.json / classification.json / report.md
 */
export async function scan(client: Client, options: ScanOptions = {}): Promise<ScanResult> {
  const { outDir, ...fuzzOptions } = options;

  const inventory = await enumerate(client);
  const fuzzResults = await fuzz(client, inventory, fuzzOptions);
  const classification = classify(inventory, fuzzResults);
  const reportMarkdown = renderMarkdown({
    inventory,
    classification,
    fuzz: fuzzResults,
  });

  if (outDir !== undefined) {
    fs.mkdirSync(outDir, { recursive: true });
    fs.writeFileSync(path.join(outDir, "inventory.json"), `${JSON.stringify(inventory, null, 2)}\n`);
    fs.writeFileSync(path.join(outDir, "fuzz.json"), `${JSON.stringify(fuzzResults, null, 2)}\n`);
    fs.writeFileSync(
      path.join(outDir, "classification.json"),
      `${JSON.stringify(classification, null, 2)}\n`,
    );
    const md = reportMarkdown.endsWith("\n") ? reportMarkdown : `${reportMarkdown}\n`;
    fs.writeFileSync(path.join(outDir, "report.md"), md);
  }

  return { inventory, fuzz: fuzzResults, classification, reportMarkdown };
}
