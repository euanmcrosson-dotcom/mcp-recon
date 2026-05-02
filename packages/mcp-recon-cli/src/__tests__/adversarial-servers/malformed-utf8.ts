/**
 * Adversarial fixture: emits a `tools/list` JSON-RPC frame whose
 * line contains lone-surrogate / invalid UTF-8 byte sequences inside
 * a tool description.
 *
 * Property tested: JSON deserialization error handling. The MCP SDK
 * client wraps each line through `JSON.parse(line)`. If the bytes
 * decode as a valid JSON string but contain unpaired surrogates,
 * `JSON.parse` accepts them — but downstream consumers (Markdown
 * report, terminal output) may still misbehave. mcp-recon should
 * surface the tool but not corrupt the report.
 *
 * Implementation note: we hand-construct the response bytes via
 * `process.stdout.write(Buffer.from(...))` so we can inject raw
 * 0x80..0xFF bytes that would otherwise be re-encoded by Node's
 * string→stdout pipeline.
 */
import { initializeResponse, type JsonRpcRequest } from "./_lib.js";

let buffer = "";
process.stdin.setEncoding("utf8");

function send(obj: unknown): void {
  process.stdout.write(`${JSON.stringify(obj)}\n`);
}

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

    if (req.method === "initialize") {
      send(initializeResponse(req.id ?? 0));
    } else if (req.method === "notifications/initialized") {
      // no response
    } else if (req.method === "tools/list") {
      // Hand-build the response with explicit invalid-UTF-8 bytes
      // smuggled inside the description string. We use \uXXXX
      // escapes for lone surrogates (which JSON.parse accepts but
      // which are not valid UTF-16 code points), plus raw high
      // bytes inserted as a Buffer.
      const id = JSON.stringify(req.id ?? 0);
      const head =
        `{"jsonrpc":"2.0","id":${id},"result":{"tools":[{"name":"mojibake","description":"`;
      const tail =
        `","inputSchema":{"type":"object","properties":{},"required":[]}}]}}\n`;
      // 0xC3 0x28: invalid 2-byte UTF-8 sequence (continuation byte
      // missing high bit). When read as UTF-8 the string contains a
      // replacement character; the JSON parser sees a valid string.
      // We also splice in a JSON-escaped lone surrogate.
      const escapedSurrogate = "lone-surrogate-\\uD800-here";
      process.stdout.write(Buffer.from(head + escapedSurrogate, "utf8"));
      process.stdout.write(Buffer.from([0xc3, 0x28, 0xa0, 0xa1]));
      process.stdout.write(Buffer.from(tail, "utf8"));
    } else {
      send({
        jsonrpc: "2.0",
        id: req.id ?? 0,
        error: { code: -32601, message: "method not implemented" },
      });
    }
  }
});
process.stdin.on("end", () => process.exit(0));
