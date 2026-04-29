/**
 * Caveat synthesizer — the bridge from mcp-recon to capnagent.
 *
 * For each classified tool, generate a copy-pasteable capnagent
 * caveat string that bounds the tool's authority to the smallest
 * surface that preserves utility. The caveat is always a *suggestion*;
 * the operator should review and tighten further to fit their
 * deployment.
 *
 * Convention: every suggestion is a single DSL predicate that can be
 * passed to `Issuer.issue(...).caveat(...)` or `cap.attenuate(...)`.
 * The caveat language is documented in capnagent's
 * `caveat_dsl.rs`.
 */

import type { ToolFacts } from "../fuzz/schema.js";
import type { AuthorityLevel, DataClass } from "./types.js";

export interface CaveatInput {
  tool: string;
  data_class: DataClass;
  authority_level: AuthorityLevel;
  facts: ToolFacts;
}

/**
 * Suggest a capnagent caveat for the given classification + facts.
 * The suggestion places `<placeholder>` markers where the operator
 * must substitute deployment-specific values.
 */
export function synthesizeCaveat(input: CaveatInput): string {
  const { tool, data_class, authority_level, facts } = input;

  // Privileged tools: deny outright. Leaving exec/shell exposed is
  // almost never the right answer; if it is, the operator can
  // hand-write a tighter argv-allowlist caveat.
  if (authority_level === "privileged") {
    return `tool != "${tool}"  // PRIVILEGED — recommend deny outright; operator should hand-write argv allowlist if exposing`;
  }

  // Destructive tools: caller-bind + bounded args + expiry.
  if (authority_level === "destructive") {
    const argClauses = boundedArgClauses(tool, facts, "destructive");
    return [
      `tool == "${tool}"`,
      `caller == "<your-caller-id>"`,
      ...argClauses,
      `now <= @<your-cap-expiry>`,
    ].join(" AND ");
  }

  // Per-class write/read defaults.
  const argClauses = boundedArgClauses(tool, facts, authority_level);
  const baseClauses = [
    `tool == "${tool}"`,
    `caller == "<your-caller-id>"`,
    ...argClauses,
    `now <= @<your-cap-expiry>`,
  ];

  // Add a class-flavoured comment so the operator knows what
  // surface they're bounding.
  const flavor = flavorComment(data_class, authority_level);
  return `${baseClauses.join(" AND ")}  // ${flavor}`;
}

function boundedArgClauses(
  _tool: string,
  facts: ToolFacts,
  authority: AuthorityLevel,
): string[] {
  const clauses: string[] = [];

  for (const arg of facts.args) {
    const argName = arg.path[0];
    if (argName === undefined) continue;

    if (arg.isPathShaped && arg.declaredType === "string") {
      // For path-shaped args, the recommended caveat is the same
      // shape capnagent's mcp-fs-agent uses post-v0.5.
      clauses.push(`arg.${argName} starts_with "<your-sandbox-prefix>/"`);
      continue;
    }
    if (arg.isUrlShaped && arg.declaredType === "string") {
      clauses.push(`arg.${argName} == "https://<your-allowed-origin>"`);
      continue;
    }
    if (arg.isCommandShaped && arg.declaredType === "string") {
      clauses.push(`arg.${argName} starts_with "<your-allowed-command-prefix>"`);
      continue;
    }
    if (arg.enumValues !== undefined) {
      // Enums constrain themselves; no extra caveat needed in v0.1.
      continue;
    }
    if (
      authority === "destructive" &&
      (arg.declaredType === "string" || arg.declaredType === "unknown")
    ) {
      // Destructive + free-form string arg = double-bound.
      clauses.push(`arg.${argName} starts_with "<your-bounded-prefix>/"`);
    }
  }

  return clauses;
}

function flavorComment(dc: DataClass, auth: AuthorityLevel): string {
  switch (dc) {
    case "filesystem":
      return `${auth.toUpperCase()} filesystem; bound the sandbox prefix tightly (capnagent round 07/10)`;
    case "network":
      return `${auth.toUpperCase()} network; allowlist exact origins (capnagent round 05/09)`;
    case "shell":
      return `${auth.toUpperCase()} shell; argv allowlist mandatory (capnagent shell-agent example)`;
    case "payments":
      return `${auth.toUpperCase()} payments; bound amount + merchant + caller`;
    case "messaging":
      return `${auth.toUpperCase()} messaging; bound recipient list + caller`;
    case "system":
      return `${auth.toUpperCase()} system; usually safe but verify env-var exposure`;
    case "metadata":
      return `${auth.toUpperCase()} metadata`;
    case "unknown":
      return `${auth.toUpperCase()} unclassified — operator must review`;
  }
}
