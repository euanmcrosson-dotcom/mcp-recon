/**
 * Adversarial fixture: returns 10,000 tools in a single
 * `tools/list` response.
 *
 * Property tested: bounded enumeration. mcp-recon should either
 * produce an inventory with all 10k tools (if no cap is desired) or
 * truncate at a documented limit. It must not OOM, hang, or silently
 * drop tools.
 */
import {
	type JsonRpcRequest,
	initializeResponse,
	runStdioLoop,
} from "./_lib.js";

const TOOL_COUNT = 10_000;

const tools = Array.from({ length: TOOL_COUNT }, (_, i) => ({
	name: `flood_tool_${i}`,
	description: `synthetic tool #${i}`,
	inputSchema: { type: "object", properties: {}, required: [] },
}));

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
