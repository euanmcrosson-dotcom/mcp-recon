/**
 * Path-traversal axis.
 *
 * For each path-shaped arg (heuristic: name contains `path`, `file`,
 * `dir`, etc. — see schema.ts::looksPathy), send classic traversal
 * payloads. Tests whether the server canonicalises paths before use.
 *
 * Per docs/SPEC.md §"Fuzzing strategy".
 */

import type { Prng } from "../prng.js";
import type { ToolFacts } from "../schema.js";

const TRAVERSAL_PAYLOADS: Array<{ value: string; reason: string }> = [
  { value: "../etc/passwd", reason: "POSIX dot-dot traversal" },
  { value: "..\\..\\windows\\system32\\config\\sam", reason: "Windows dot-dot traversal" },
  { value: "/etc/passwd", reason: "absolute POSIX path outside sandbox" },
  { value: "C:\\Windows\\System32\\drivers\\etc\\hosts", reason: "absolute Windows path" },
  { value: "/sandbox/../../../etc/passwd", reason: "in-sandbox prefix + traversal" },
  { value: "/sandbox/%2e%2e/etc/passwd", reason: "percent-encoded traversal in path" },
  { value: "//etc/passwd", reason: "double-slash absolute" },
  { value: "/", reason: "filesystem root" },
  { value: "~/.ssh/id_rsa", reason: "tilde-expansion private key" },
  { value: "\\\\.\\PhysicalDrive0", reason: "Windows raw device path" },
  { value: "file:///etc/passwd", reason: "file:// URI" },
];

/** Generate path-traversal inputs for one tool. */
export function* generatePathTraversal(
  facts: ToolFacts,
  _prng: Prng,
): Generator<{ args: Record<string, unknown>; rationale: string }> {
  if (facts.isArgless) return;

  for (const arg of facts.args) {
    const argName = arg.path[0];
    if (argName === undefined) continue;
    if (!arg.isPathShaped) continue;
    if (arg.declaredType !== "string" && arg.declaredType !== "unknown") continue;

    const baseArgs = baseShape(facts);
    for (const { value, reason } of TRAVERSAL_PAYLOADS) {
      yield {
        args: { ...baseArgs, [argName]: value },
        rationale: `path-traversal "${argName}": ${reason}`,
      };
    }
  }
}

function baseShape(facts: ToolFacts): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const arg of facts.args) {
    const argName = arg.path[0];
    if (argName === undefined) continue;
    if (!arg.required) continue;
    out[argName] = arg.declaredType === "string" ? "x" : 0;
  }
  return out;
}
