/**
 * Tiny shared scaffolding for adversarial MCP-stdio fixtures.
 *
 * Each fixture below speaks only as much of the MCP wire protocol as
 * it needs to be enumerable (`initialize` + `tools/list`). We use a
 * raw line-oriented JSON-RPC reader rather than the MCP SDK's
 * `Server` class because several fixtures must intentionally violate
 * the schema or emit non-JSON bytes — the SDK's response path won't
 * let us do that.
 *
 * NOT a real MCP server. NOT for production. This file lives in
 * __tests__/ for a reason.
 */

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id?: number | string | null;
  method: string;
  params?: unknown;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number | string | null;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

/**
 * Minimal handshake response — what every adversarial fixture must
 * answer to `initialize` so the client gets past the handshake. The
 * adversarial behavior is layered on top of `tools/list`.
 */
export const INITIALIZE_RESULT = {
  protocolVersion: "2024-11-05",
  capabilities: { tools: {} },
  serverInfo: { name: "adversarial-test-server", version: "0.0.0" },
};

/**
 * Drive a stdin → handler → stdout loop. The handler may return a
 * single response, an array of responses, or `undefined` to stay
 * silent (notifications, or the slow-response fixture).
 */
export function runStdioLoop(
  handler: (req: JsonRpcRequest) => Promise<unknown> | unknown,
): void {
  let buffer = "";

  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => {
    buffer += chunk;
    let nl: number;
    // eslint-disable-next-line no-cond-assign
    while ((nl = buffer.indexOf("\n")) !== -1) {
      const line = buffer.slice(0, nl).replace(/\r$/, "");
      buffer = buffer.slice(nl + 1);
      if (line.length === 0) continue;
      let req: JsonRpcRequest;
      try {
        req = JSON.parse(line) as JsonRpcRequest;
      } catch {
        continue;
      }
      void Promise.resolve(handler(req)).then((out) => {
        if (out === undefined) return;
        const arr = Array.isArray(out) ? out : [out];
        for (const msg of arr) {
          process.stdout.write(`${JSON.stringify(msg)}\n`);
        }
      });
    }
  });
  process.stdin.on("end", () => {
    process.exit(0);
  });
}

/** Standard `initialize` response builder. */
export function initializeResponse(id: number | string | null): JsonRpcResponse {
  return { jsonrpc: "2.0", id: id ?? 0, result: INITIALIZE_RESULT };
}
