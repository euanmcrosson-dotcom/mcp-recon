/**
 * Adversarial fixture: accepts the `initialize` handshake, but never
 * answers `tools/list`. The process stays alive until killed.
 *
 * Property tested: timeout / wall-clock. mcp-recon must enforce a
 * per-request timeout so a hostile server cannot hang the operator
 * forever. The MCP SDK's default request timeout is 60s; mcp-recon's
 * `enumerate` should accept a shorter cap (or the integration test
 * passes one explicitly).
 */
import { initializeResponse, runStdioLoop, type JsonRpcRequest } from "./_lib.js";

runStdioLoop((req: JsonRpcRequest) => {
  if (req.method === "initialize") return initializeResponse(req.id ?? 0);
  if (req.method === "notifications/initialized") return undefined;
  if (req.method === "tools/list") {
    // Intentionally never reply. The client should time out.
    return undefined;
  }
  return {
    jsonrpc: "2.0",
    id: req.id ?? 0,
    error: { code: -32601, message: "method not implemented" },
  };
});
