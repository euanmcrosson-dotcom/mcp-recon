/**
 * `@mcp-recon/cli` — public library surface.
 *
 * The CLI binary in `bin/recon.ts` is the daily-driver. This module
 * re-exports the same primitives so other tools can compose them
 * without spawning the CLI as a subprocess.
 *
 * v0.1 surface (per docs/SPEC.md):
 *
 *   - parseServerSpec(spec)           — string → ServerSpec
 *   - openClient(spec)                — ServerSpec → connected SDK Client
 *   - enumerate(client)               → ToolInventory (one tool inventory document)
 *   - fuzz(client, inventory)         → FuzzResults (one fuzz document)
 *
 * v0.1 weeks 3+ will add `classify`, `report`, `scan` exports here.
 */

export {
	type ServerSpec,
	parseServerSpec,
	openClient,
	closeClient,
} from "./transport.js";

export {
	type ToolInventory,
	type EnumeratedTool,
	type EnumerateOptions,
	enumerate,
	INVENTORY_SCHEMA,
	DEFAULT_ENUMERATE_TIMEOUT_MS,
	DEFAULT_MAX_DESCRIPTION_CHARS,
	TRUNCATION_MARKER,
} from "./enumerate.js";

export {
	type FuzzAxis,
	type FuzzCall,
	type FuzzOutcome,
	type FuzzResults,
	type FuzzToolSummary,
	type FuzzOptions,
	fuzz,
	FUZZ_SCHEMA,
} from "./fuzz/index.js";

export {
	type AuthorityLevel,
	type Classification,
	type ClassificationResults,
	type DataClass,
	classify,
	noisyOr,
	synthesizeCaveat,
	CLASSIFICATION_SCHEMA,
} from "./classify/index.js";

export {
	type RenderInput,
	renderMarkdown,
} from "./report/index.js";

export {
	type CaveatBindings,
	type CaveatPlan,
	type CaveatsResults,
	type FlagReason,
	planCaveats,
	CAVEATS_SCHEMA,
} from "./caveats/index.js";

export { renderCaveatsMarkdown } from "./caveats/render.js";

export {
	type ScanResult,
	type ScanOptions,
	scan,
} from "./scan/index.js";
