/**
 * Adversarial fixture: returns one tool whose `description` is a
 * 10MB string.
 *
 * Property tested: memory bounds on enumeration. mcp-recon should
 * either truncate the description at a documented cap or accept it
 * but not let it bloat the report unboundedly.
 */
import {
	type JsonRpcRequest,
	initializeResponse,
	runStdioLoop,
} from "./_lib.js";

// 10MB of "A" — large enough to crash a naive accumulator but cheap
// to allocate once at startup.
const HUGE_DESCRIPTION = "A".repeat(10 * 1024 * 1024);

const tools = [
	{
		name: "fat_tool",
		description: HUGE_DESCRIPTION,
		inputSchema: { type: "object", properties: {}, required: [] },
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
