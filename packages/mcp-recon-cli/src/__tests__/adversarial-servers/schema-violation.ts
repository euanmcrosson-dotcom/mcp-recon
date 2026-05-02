/**
 * Adversarial fixture: returns a `tools/list` response that
 * violates the MCP schema in several ways at once.
 *
 * The MCP `Tool` type requires `name: string` and
 * `inputSchema: object`. We send tools that:
 *   - omit the required `name` field
 *   - have a numeric `name`
 *   - have a string `inputSchema` instead of an object
 *   - replace the entire `tools` array with a non-array value
 *
 * We rotate through these per call so a single fixture can probe
 * multiple shapes; the integration test asserts that mcp-recon
 * either rejects the response loudly or sanitizes it — what it
 * MUST NOT do is silently accept malformed entries and pass them
 * downstream where they will crash the report renderer.
 *
 * Note: the MCP SDK's Zod schema validates the response on the
 * client side, so for several variants the call will reject before
 * mcp-recon's enumerate code even sees the result. That's the
 * expected outcome — the test asserts the rejection rather than
 * the bypass.
 */
import { initializeResponse, runStdioLoop, type JsonRpcRequest } from "./_lib.js";

runStdioLoop((req: JsonRpcRequest) => {
  if (req.method === "initialize") return initializeResponse(req.id ?? 0);
  if (req.method === "notifications/initialized") return undefined;
  if (req.method === "tools/list") {
    // Multiple violations in one response. Pick the most aggressive:
    // tools is a non-array.
    return {
      jsonrpc: "2.0",
      id: req.id ?? 0,
      result: {
        tools: "not-an-array",
      },
    };
  }
  return {
    jsonrpc: "2.0",
    id: req.id ?? 0,
    error: { code: -32601, message: "method not implemented" },
  };
});
