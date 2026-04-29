/**
 * URL-hostility axis.
 *
 * For each URL-shaped arg (heuristic: name contains `url`, `origin`,
 * `endpoint`, etc.), send URL-shaped payloads designed to confuse
 * URL parsers and canonicalisers.
 *
 * Per docs/SPEC.md §"Fuzzing strategy".
 */

import type { Prng } from "../prng.js";
import type { ToolFacts } from "../schema.js";

const URL_PAYLOADS: Array<{ value: string; reason: string }> = [
  { value: "https://attacker.example@victim.example/", reason: "userinfo-splitting" },
  { value: "https://victim.example/@attacker.example/", reason: "fake-userinfo segment" },
  // Cyrillic-а homograph in `аpi.example.com`
  { value: "https://аpi.example.com/x", reason: "Cyrillic-a IDN homograph" },
  { value: "https://xn--pi-6kc.example.com/x", reason: "punycode IDN form" },
  { value: "javascript:alert(1)", reason: "javascript: scheme" },
  { value: "data:text/plain;base64,SGVsbG8=", reason: "data: scheme" },
  { value: "file:///etc/passwd", reason: "file:// scheme" },
  { value: "http://localhost:80/", reason: "loopback default port" },
  { value: "http://127.0.0.1/", reason: "loopback IP literal" },
  { value: "http://169.254.169.254/latest/meta-data/", reason: "AWS instance metadata" },
  { value: "http://[::]/", reason: "IPv6 unspecified" },
  { value: "http://0.0.0.0/", reason: "IPv4 unspecified" },
  { value: "http://example.com\\.attacker.com/", reason: "backslash-host trick" },
  { value: "http:////example.com/x", reason: "extra slashes" },
  { value: "not-a-url-at-all", reason: "unparseable string" },
];

/** Generate URL-hostility inputs for one tool. */
export function* generateUrlHostility(
  facts: ToolFacts,
  _prng: Prng,
): Generator<{ args: Record<string, unknown>; rationale: string }> {
  if (facts.isArgless) return;

  for (const arg of facts.args) {
    const argName = arg.path[0];
    if (argName === undefined) continue;
    if (!arg.isUrlShaped) continue;
    if (arg.declaredType !== "string" && arg.declaredType !== "unknown") continue;

    const baseArgs = baseShape(facts);
    for (const { value, reason } of URL_PAYLOADS) {
      yield {
        args: { ...baseArgs, [argName]: value },
        rationale: `url-hostility "${argName}": ${reason}`,
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
