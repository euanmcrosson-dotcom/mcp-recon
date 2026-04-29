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
 *
 * v0.2+ will add `fuzz`, `classify`, `report`, `scan` exports here.
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
  enumerate,
  INVENTORY_SCHEMA,
} from "./enumerate.js";
