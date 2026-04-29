/**
 * Rule table — the single source of truth the classifier reads.
 *
 * Every rule is a triple: (pattern, data-class, authority-level)
 * plus a confidence weight. Rules over tool *names* and *descriptions*
 * use regex match; rules over *schemas* use heuristic shape detection
 * already implemented in `../fuzz/schema.ts`.
 *
 * Per docs/METHODOLOGY.md §"Confidence scoring":
 *   - Tool name match           — 0.7 (strong cue, gameable)
 *   - Description match         — 0.5 (helpful, often missing)
 *   - Schema match              — 0.4 (structural, gameable too)
 *   - Side-effect verb in desc  — 0.6 (raises authority by one step)
 *
 * Adding a rule? Update this file AND docs/METHODOLOGY.md's change-log
 * with date + reason. No silent drift.
 */

import type { AuthorityLevel, DataClass } from "./types.js";

/** Single rule predicting (data_class, authority_level) given a name/description match. */
export interface NameOrDescRule {
  /** Regex pattern (case-insensitive). */
  pattern: RegExp;
  data_class: DataClass;
  /**
   * Authority floor — the rule asserts at least this authority level.
   * Final authority can be escalated by side-effect verbs.
   */
  authority_floor: AuthorityLevel;
  /** Where this rule applies. */
  scope: "name" | "description" | "either";
}

/** Confidence weights per rule scope. */
export const NAME_MATCH_WEIGHT = 0.7;
export const DESCRIPTION_MATCH_WEIGHT = 0.5;
export const SCHEMA_MATCH_WEIGHT = 0.4;
/**
 * Bonus added when fuzz results show the tool actually accepts adversarial
 * inputs (i.e. > 1 ok response in the fuzz stream). Caps confidence higher.
 */
export const FUZZ_INFORMED_BONUS = 0.1;

/** Side-effect verbs that raise authority by one step when seen in description. */
export const SIDE_EFFECT_VERBS = [
  /\b(write|create|insert|append|update|modify|edit|set|put|post|patch)\b/i,
  /\b(delete|remove|drop|unlink|destroy|cancel|revoke)\b/i,
  /\b(send|email|notify|page|alert|publish|broadcast)\b/i,
  /\b(spawn|exec|launch|run|invoke|fork|shell)\b/i,
  /\b(charge|pay|transfer|wire|refund|debit|credit)\b/i,
];

/**
 * Side-effect verbs strong enough to assert *destructive* directly
 * (matches push authority to "destructive", not just "write").
 */
export const DESTRUCTIVE_VERBS = [
  /\b(delete|remove|drop|unlink|destroy|cancel|revoke|wipe|purge|truncate)\b/i,
];

/**
 * Side-effect verbs strong enough to assert *privileged* directly
 * (subprocess spawn / shell execution).
 */
export const PRIVILEGED_VERBS = [/\b(spawn|exec|shell|bash|cmd|invoke_program|fork)\b/i];

/**
 * Rule list. Ordered from most specific to most general — the
 * classifier evaluates all rules; this ordering is for human review.
 */
export const RULES: readonly NameOrDescRule[] = [
  // ─── filesystem ──────────────────────────────────────────────
  {
    pattern: /\b(read[_-]?(file|text[_-]?file|media[_-]?file|multiple[_-]?files))\b/i,
    data_class: "filesystem",
    authority_floor: "read",
    scope: "name",
  },
  {
    pattern: /\b(list[_-]?(directory|directory[_-]?with[_-]?sizes)|directory[_-]?tree)\b/i,
    data_class: "filesystem",
    authority_floor: "read",
    scope: "name",
  },
  {
    pattern: /\b(get[_-]?file[_-]?info|search[_-]?files|list[_-]?allowed[_-]?directories)\b/i,
    data_class: "filesystem",
    authority_floor: "read",
    scope: "name",
  },
  {
    pattern: /\b(write[_-]?file|edit[_-]?file|create[_-]?directory|move[_-]?file)\b/i,
    data_class: "filesystem",
    authority_floor: "write",
    scope: "name",
  },
  {
    pattern: /\b(delete[_-]?(path|file|directory|dir))\b/i,
    data_class: "filesystem",
    authority_floor: "destructive",
    scope: "name",
  },
  {
    pattern: /\b(file|directory|folder|filesystem|filepath|dirent)\b/i,
    data_class: "filesystem",
    authority_floor: "read",
    scope: "description",
  },
  // ─── network ─────────────────────────────────────────────────
  {
    pattern: /\b(http[_-]?(get|post|put|delete)|fetch|request)\b/i,
    data_class: "network",
    authority_floor: "read",
    scope: "name",
  },
  {
    pattern: /\b(http|https|fetch|request|webhook|api[_-]?call|url|origin)\b/i,
    data_class: "network",
    authority_floor: "read",
    scope: "description",
  },
  // ─── shell ──────────────────────────────────────────────────
  {
    pattern: /\b(exec|shell|run[_-]?command|spawn|process)\b/i,
    data_class: "shell",
    authority_floor: "privileged",
    scope: "name",
  },
  {
    pattern: /\b(execute|shell|subprocess|invoke|launch[_-]?process)\b/i,
    data_class: "shell",
    authority_floor: "privileged",
    scope: "description",
  },
  // ─── payments ───────────────────────────────────────────────
  {
    pattern: /\b(charge|refund|purchase|wire|payment|invoice|debit|credit)\b/i,
    data_class: "payments",
    authority_floor: "write",
    scope: "either",
  },
  // ─── messaging ──────────────────────────────────────────────
  {
    pattern: /\b(send[_-]?(email|message|sms|notification)|notify|publish|broadcast)\b/i,
    data_class: "messaging",
    authority_floor: "write",
    scope: "name",
  },
  {
    pattern: /\b(email|notification|message|chat|page|alert)\b/i,
    data_class: "messaging",
    authority_floor: "read",
    scope: "description",
  },
  // ─── system ─────────────────────────────────────────────────
  {
    pattern: /\b(get[_-]?env|environ(ment)?|process[_-]?(info|status)|sysinfo|system[_-]?info|hostname)\b/i,
    data_class: "system",
    authority_floor: "read",
    scope: "either",
  },
  // ─── metadata / read-only ───────────────────────────────────
  {
    pattern: /\b(read[_-]?graph|search[_-]?nodes|open[_-]?nodes)\b/i,
    data_class: "metadata",
    authority_floor: "read",
    scope: "name",
  },
  {
    pattern: /\b(echo|get[_-]?(structured[_-]?content|annotated[_-]?message|resource[_-]?(links|reference)|sum|tiny[_-]?image))\b/i,
    data_class: "metadata",
    authority_floor: "read",
    scope: "name",
  },
  {
    pattern: /\b(toggle[_-]?(simulated[_-]?logging|subscriber[_-]?updates))\b/i,
    data_class: "system",
    authority_floor: "write",
    scope: "name",
  },
  // ─── memory-server CRUD on a knowledge graph ─────────────────
  {
    pattern: /\b(create[_-]?(entities|relations))\b/i,
    data_class: "metadata",
    authority_floor: "write",
    scope: "name",
  },
  {
    pattern: /\b(add[_-]?observations)\b/i,
    data_class: "metadata",
    authority_floor: "write",
    scope: "name",
  },
  {
    pattern: /\b(delete[_-]?(entities|relations|observations))\b/i,
    data_class: "metadata",
    authority_floor: "destructive",
    scope: "name",
  },
  // ─── thinking / opaque reasoning tools ──────────────────────
  {
    pattern: /\b(sequentialthinking|reason|plan|think)\b/i,
    data_class: "metadata",
    authority_floor: "read",
    scope: "name",
  },
  // ─── trigger-shaped + simulate-shaped (test surfaces) ────────
  {
    pattern: /\b(trigger[_-]?(long[_-]?running[_-]?operation)?|simulate[_-]?(research[_-]?query))\b/i,
    data_class: "metadata",
    authority_floor: "write",
    scope: "name",
  },
];

/** Rank the four authority levels so we can take the strongest signal. */
const AUTHORITY_RANK: Record<AuthorityLevel, number> = {
  read: 0,
  write: 1,
  destructive: 2,
  privileged: 3,
};

/** Return the higher of two authority levels (lattice join). */
export function maxAuthority(a: AuthorityLevel, b: AuthorityLevel): AuthorityLevel {
  return AUTHORITY_RANK[a] >= AUTHORITY_RANK[b] ? a : b;
}

/** Escalate authority by one step (read → write → destructive → privileged). */
export function escalateAuthority(a: AuthorityLevel): AuthorityLevel {
  switch (a) {
    case "read":
      return "write";
    case "write":
      return "destructive";
    case "destructive":
      return "privileged";
    case "privileged":
      return "privileged"; // saturates
  }
}
