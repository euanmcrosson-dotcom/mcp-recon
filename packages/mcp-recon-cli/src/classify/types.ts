/**
 * Wire-format types for the v0.1 classification document.
 *
 * Schema tag: `mcp-recon/v0.1/classification`. Every field's design
 * intent is documented in docs/METHODOLOGY.md.
 */

/** Schema-version tag for classification documents. */
export const CLASSIFICATION_SCHEMA = "mcp-recon/v0.1/classification" as const;

/** Top-level data classes, per docs/METHODOLOGY.md. */
export type DataClass =
  | "filesystem"
  | "network"
  | "shell"
  | "payments"
  | "messaging"
  | "system"
  | "metadata"
  | "unknown";

/** Authority levels, per docs/METHODOLOGY.md. */
export type AuthorityLevel = "read" | "write" | "destructive" | "privileged";

/** One classification per tool. */
export interface Classification {
  /** Tool name from the inventory. */
  tool: string;
  /** Assigned data-class. */
  data_class: DataClass;
  /** Assigned authority level. */
  authority_level: AuthorityLevel;
  /**
   * The load-bearing flag: tool takes user-controllable string args
   * AND has write/destructive/privileged authority.
   */
  confused_deputy_candidate: boolean;
  /** Combined confidence in [0, 1] per noisy-OR over fired rules. */
  confidence: number;
  /** Free-form rationale listing every rule that fired and its weight. */
  rationale: string;
  /** Suggested capnagent caveat for this tool (string, copy-pasteable). */
  recommended_caveat: string;
}

/** Top-level classification document. */
export interface ClassificationResults {
  schema: typeof CLASSIFICATION_SCHEMA;
  scanned_at: string;
  server: { name?: string; version?: string };
  /** Whether fuzz results were folded into confidence (raises confidence by 0.1). */
  fuzz_informed: boolean;
  classifications: Classification[];
}
