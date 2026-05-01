/**
 * Adversarial fixture: returns one tool whose `inputSchema` contains
 * a `$ref` cycle (a schema that refers to itself indefinitely).
 *
 * Property tested: schema-walking robustness. Anything that
 * recursively walks `inputSchema` (the future fuzz schema-drive, the
 * report renderer expanding nested args) must terminate.
 *
 * Note: the schema below is not literally a cyclic JS object — it's
 * a JSON Schema that uses `$ref` self-reference. Naive `$ref`
 * resolvers that don't track seen-paths will recurse forever; mature
 * resolvers will short-circuit on revisiting the same `$ref`.
 */
import { initializeResponse, runStdioLoop, type JsonRpcRequest } from "./_lib.js";

const tools = [
  {
    name: "ouroboros",
    description: "input schema is self-referential via $ref",
    inputSchema: {
      type: "object",
      properties: {
        node: { $ref: "#/definitions/Node" },
      },
      definitions: {
        Node: {
          type: "object",
          properties: {
            child: { $ref: "#/definitions/Node" },
            value: { type: "string" },
          },
        },
      },
    },
  },
];

runStdioLoop((req: JsonRpcRequest) => {
  if (req.method === "initialize") return initializeResponse(req.id ?? 0);
  if (req.method === "notifications/initialized") return undefined;
  if (req.method === "tools/list") {
    return { jsonrpc: "2.0", id: req.id ?? 0, result: { tools } };
  }
  return {
    jsonrpc: "2.0",
    id: req.id ?? 0,
    error: { code: -32601, message: "method not implemented" },
  };
});
