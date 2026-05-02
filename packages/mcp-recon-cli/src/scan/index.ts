/**
 * `scan` — one-shot orchestrator: enumerate → fuzz → classify → report.
 *
 * The daily-driver command. Composes the four primitives defined in
 * `enumerate.ts`, `fuzz/index.ts`, `classify/index.ts`, `report/index.ts`
 * and writes all four artefacts to a single output directory.
 *
 * When operator bindings are supplied (caller / sandbox-prefix / expiry /
 * per-tool overrides), `scan` also emits a 5th artefact, `caveats.json`,
 * by running the classification through the caveats planner. This closes
 * the bridge to capnagent in a single command. Without bindings, `scan`
 * keeps its original 4-artefact behaviour intact.
 *
 * Returns the bag of intermediate values so a caller (CLI or library)
 * can post-process beyond the on-disk artefacts.
 */

import * as fs from "node:fs";
import * as path from "node:path";

import type { Client } from "@modelcontextprotocol/sdk/client/index.js";

import { planCaveats } from "../caveats/index.js";
import type { CaveatBindings, CaveatsResults } from "../caveats/types.js";
import { classify } from "../classify/index.js";
import type { ClassificationResults } from "../classify/types.js";
import { enumerate } from "../enumerate.js";
import type { ToolInventory } from "../enumerate.js";
import { fuzz, type FuzzOptions } from "../fuzz/index.js";
import type { FuzzResults } from "../fuzz/types.js";
import { renderMarkdown } from "../report/index.js";

export interface ScanResult {
  inventory: ToolInventory;
  fuzz: FuzzResults;
  classification: ClassificationResults;
  reportMarkdown: string;
  /** Present when bindings triggered caveats emission. */
  caveats?: CaveatsResults;
}

export interface ScanOptions extends FuzzOptions {
  /** If set, write the artefacts to this directory. */
  outDir?: string;
  /**
   * If set AND has at least one of caller / sandbox_prefix / expiry /
   * per_tool_overrides populated, scan additionally produces a caveats
   * document (and writes `caveats.json` when `outDir` is set). Empty
   * objects, or objects with only undefined values, do not trigger
   * caveats emission.
   */
  bindings?: CaveatBindings;
}

/**
 * Returns true if the bindings object has at least one populated field
 * relevant to caveats emission. We treat empty strings as "set" so the
 * operator's intent is honoured (the bridge handles substitution
 * downstream and will flag empty values via `unsubstituted_placeholder`
 * if appropriate).
 */
function hasAnyBinding(bindings: CaveatBindings): boolean {
  if (bindings.caller !== undefined) return true;
  if (bindings.sandbox_prefix !== undefined) return true;
  if (bindings.expiry !== undefined) return true;
  if (bindings.per_tool_overrides !== undefined) {
    if (Object.keys(bindings.per_tool_overrides).length > 0) return true;
  }
  return false;
}

/**
 * Run the full pipeline. Caller owns the client (open + close).
 *
 * Side effects (only when `outDir` is provided):
 *   - mkdir -p outDir
 *   - write inventory.json / fuzz.json / classification.json / report.md
 *   - write caveats.json IFF bindings provided with at least one field set
 */
export async function scan(client: Client, options: ScanOptions = {}): Promise<ScanResult> {
  const { outDir, bindings, ...fuzzOptions } = options;

  const inventory = await enumerate(client);
  const fuzzResults = await fuzz(client, inventory, fuzzOptions);
  const classification = classify(inventory, fuzzResults);
  const reportMarkdown = renderMarkdown({
    inventory,
    classification,
    fuzz: fuzzResults,
  });

  const caveats =
    bindings !== undefined && hasAnyBinding(bindings)
      ? planCaveats(classification, bindings)
      : undefined;

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
    if (caveats !== undefined) {
      fs.writeFileSync(path.join(outDir, "caveats.json"), `${JSON.stringify(caveats, null, 2)}\n`);
    }
  }

  const result: ScanResult = { inventory, fuzz: fuzzResults, classification, reportMarkdown };
  if (caveats !== undefined) result.caveats = caveats;
  return result;
}
