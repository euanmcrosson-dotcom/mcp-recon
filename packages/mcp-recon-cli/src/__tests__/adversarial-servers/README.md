# Adversarial-server fixtures

This directory contains tiny stdio-MCP-protocol-speaking server stubs that
simulate **attacker behavior against mcp-recon**. They are test fixtures,
**not real MCP servers**, and never publish to npm.

mcp-recon's job is to scan untrusted MCP servers and produce a threat
profile. These fixtures pin the behavior of `enumerate` (and, eventually,
`fuzz` and `report`) when the server is hostile — returns garbage, hangs,
emits terminal escape sequences, smuggles prompt injection through tool
descriptions, etc. The integration spec at
`../adversarial.integration.test.ts` drives each fixture end-to-end and
asserts the desired behavior.

## How a fixture works

Each fixture is a single `.ts` file run via `tsx`. It speaks just enough
of the MCP wire protocol to be enumerable:

1. Reads JSON-RPC requests one-per-line from stdin.
2. Answers `initialize` with a benign capabilities response (so the
   client gets past the handshake).
3. Implements its adversarial behavior on `tools/list` (or refuses to
   answer it, in the slow-response case).
4. Returns method-not-implemented for anything else.

The shared scaffolding lives in `_lib.ts`. We do **not** use the MCP
SDK's `Server` class, because several fixtures must intentionally
violate the schema or emit raw bytes outside the JSON-RPC frame — the
SDK's response path won't let us do that.

## Fixture catalog

| Fixture | Attack | mcp-recon property tested |
| --- | --- | --- |
| `huge-tool-list.ts` | 10,000 tools in one `tools/list` response | bounded enumeration / no OOM |
| `giant-tool-description.ts` | one tool with a 10MB description | memory bound; report truncates |
| `recursive-schema.ts` | `inputSchema` with a `$ref` cycle | schema-walking robustness — no infinite recursion |
| `malformed-utf8.ts` | invalid UTF-8 bytes / lone surrogates inside a description | JSON deserialization error handling |
| `slow-response.ts` | accepts `tools/list` then never replies | per-request timeout / wall-clock bound |
| `process-control-bytes.ts` | ANSI escapes + BEL bytes between JSON-RPC lines | stdio-frame strictness; control bytes never reach the operator's terminal |
| `description-injection.ts` | prompt-injection-shaped strings inside descriptions | report integrity — attacker text framed as data, not authoritative |
| `schema-violation.ts` | `tools` field is a string instead of an array | strict-MCP enforcement |

## How to add a new fixture

1. Drop a new `.ts` file in this directory. Start from the smallest
   existing fixture (e.g. `huge-tool-list.ts`).
2. Use `runStdioLoop` from `_lib.ts` for the request loop unless you
   need to inject raw bytes outside the JSON-RPC frame, in which case
   write directly to `process.stdout` (see `malformed-utf8.ts` /
   `process-control-bytes.ts`).
3. Add a corresponding `it(...)` block to
   `../adversarial.integration.test.ts`. The test title should encode
   the attack and the asserted property:

   ```ts
   it("<fixture-name>: <one-sentence attack description> — <expected outcome>", ...)
   ```

4. Use a tight per-test timeout (≤ 30s, or ≤ 12s for slow-response-
   shaped tests). A hung adversarial server should never stall the
   suite.
5. If your fixture exposes a real bug in mcp-recon, **fix the bug
   first**, then assert the fixed behavior. Don't ship a passing
   test that masks the vulnerability.

## Running

These tests are gated behind `LIVE_MCP=1` (same as the
`server-filesystem` integration tests) so the default `npm test` run
stays hermetic:

```bash
LIVE_MCP=1 npm test -w @mcp-recon/cli
```
