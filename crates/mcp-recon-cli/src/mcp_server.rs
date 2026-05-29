//! `mcp-recon mcp-server` — turn the recon CLI into an MCP server itself.
//!
//! Newline-delimited JSON-RPC 2.0 over stdio per the MCP 2025-03-26 spec.
//! Exposes the core classifier and caveats functions as MCP tools so an
//! agent can run mcp-recon the same way it'd call any other MCP tool.
//!
//! Tools:
//! - `classify_inventory(inventory)` → findings.v1 JSON
//! - `caveats(inventory)` → mcp-recon/v0.1/caveats JSON
//!
//! stdout carries JSON-RPC frames only. Any human-facing log lines go to
//! stderr (per MCP transport contract).

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

use mcp_recon_core::{caveats_v1, classify, McpInventory};

const PROTOCOL_VERSION: &str = "2025-03-26";

#[derive(Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    /// Absent on notifications (per JSON-RPC 2.0); when absent, the server
    /// must NOT send a response.
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

/// JSON-RPC 2.0 standard error codes (subset we surface).
const PARSE_ERROR: i32 = -32700;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const INTERNAL_ERROR: i32 = -32603;

/// Run the stdio loop. Reads newline-delimited JSON-RPC from stdin, writes
/// responses to stdout, flushing after every frame so the client sees them.
pub fn run() -> Result<()> {
    eprintln!(
        "mcp-recon mcp-server: ready (stdio, MCP {PROTOCOL_VERSION}, mcp-recon {})",
        env!("CARGO_PKG_VERSION")
    );
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut buf = String::new();
    let mut handle = stdin.lock();
    loop {
        buf.clear();
        let n = handle.read_line(&mut buf)?;
        if n == 0 {
            // EOF — client closed stdin, shut down cleanly.
            break;
        }
        let line = buf.trim();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Request>(line) {
            Ok(req) => dispatch(req),
            Err(e) => Some(error_response(
                None,
                PARSE_ERROR,
                &format!("invalid JSON-RPC frame: {e}"),
            )),
        };

        if let Some(resp) = response {
            writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Route a single request. Returns `None` for notifications — per JSON-RPC
/// 2.0 a notification is any message lacking an `id`, and the server must
/// not respond. This subsumes `notifications/initialized` and every other
/// notification the protocol may carry.
fn dispatch(req: Request) -> Option<Value> {
    let id = req.id?;

    let result = match req.method.as_str() {
        "initialize" => Ok(handle_initialize()),
        "tools/list" => Ok(handle_tools_list()),
        "tools/call" => handle_tools_call(req.params),
        m => Err((METHOD_NOT_FOUND, format!("method not found: {m}"))),
    };

    Some(match result {
        Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
        Err((code, msg)) => error_response(Some(id), code, &msg),
    })
}

fn handle_initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "mcp-recon",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Tools that classify a target MCP server's tool inventory \
            against deterministic security rules and emit findings / capnagent caveats. \
            Pass each tool an `inventory` argument shaped per the mcp-recon.inventory.v1 \
            schema. No live enumeration here — supply a pre-built inventory."
    })
}

fn handle_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "classify_inventory",
                "description": "Classify an mcp-recon.inventory.v1 document against \
                    deterministic, rule-based detectors (OWASP LLM Top 10 / NIST AI RMF \
                    / MITRE ATLAS). Same inventory in, same findings out — no LLM in \
                    the loop. Returns a findings.v1 array as JSON-encoded text.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "inventory": {
                            "type": "object",
                            "description": "An mcp-recon.inventory.v1 JSON document \
                                (servers + tools)."
                        }
                    },
                    "required": ["inventory"]
                }
            },
            {
                "name": "caveats",
                "description": "Turn an mcp-recon.inventory.v1 into an \
                    `mcp-recon/v0.1/caveats` artifact — capnagent-ready deny/scope \
                    plans with caveat-DSL predicates and rule provenance for every \
                    authority-relevant tool. The Find→Bind handoff.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "inventory": {
                            "type": "object",
                            "description": "An mcp-recon.inventory.v1 JSON document."
                        }
                    },
                    "required": ["inventory"]
                }
            }
        ]
    })
}

fn handle_tools_call(params: Value) -> Result<Value, (i32, String)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((INVALID_PARAMS, "missing or non-string `name`".to_string()))?;

    // Validate the tool name BEFORE touching arguments so a typo'd tool name
    // surfaces as METHOD_NOT_FOUND (clear) rather than INVALID_PARAMS-on-
    // inventory (confusing — points the caller at the wrong layer).
    if !matches!(name, "classify_inventory" | "caveats") {
        return Err((METHOD_NOT_FOUND, format!("unknown tool: {name}")));
    }

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    let inventory_val = arguments
        .get("inventory")
        .ok_or((INVALID_PARAMS, "missing `arguments.inventory`".to_string()))?;
    let inv: McpInventory = serde_json::from_value(inventory_val.clone()).map_err(|e| {
        (
            INVALID_PARAMS,
            format!("inventory did not parse as mcp-recon.inventory.v1: {e}"),
        )
    })?;

    let text = match name {
        "classify_inventory" => {
            let findings = classify(&inv);
            serde_json::to_string_pretty(&findings)
                .map_err(|e| (INTERNAL_ERROR, format!("serialize findings failed: {e}")))?
        }
        "caveats" => {
            let art = caveats_v1(&inv);
            serde_json::to_string_pretty(&art)
                .map_err(|e| (INTERNAL_ERROR, format!("serialize caveats failed: {e}")))?
        }
        _ => unreachable!("tool name was validated above"),
    };

    Ok(json!({
        "content": [ { "type": "text", "text": text } ],
        "isError": false
    }))
}

fn error_response(id: Option<Value>, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, params: Value) -> Request {
        Request {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn initialize_reports_protocol_and_tools_capability() {
        let resp = dispatch(req("initialize", json!({}))).expect("response");
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(resp["result"]["capabilities"]["tools"].is_object());
        assert_eq!(resp["result"]["serverInfo"]["name"], "mcp-recon");
    }

    #[test]
    fn initialized_notification_yields_no_response() {
        let n = Request {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: json!({}),
        };
        assert!(dispatch(n).is_none(), "notifications must not be answered");
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let resp = dispatch(req("does/not/exist", json!({}))).expect("response");
        assert_eq!(resp["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn tools_list_advertises_both_tools() {
        let resp = dispatch(req("tools/list", json!({}))).expect("response");
        let tools = resp["result"]["tools"].as_array().expect("array");
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"classify_inventory"));
        assert!(names.contains(&"caveats"));
        // Each tool must declare an object inputSchema with a required `inventory`.
        for t in tools {
            assert_eq!(t["inputSchema"]["type"], "object");
            assert_eq!(t["inputSchema"]["required"][0], "inventory");
        }
    }

    #[test]
    fn tools_call_classify_returns_findings_text_content() {
        let resp = dispatch(req(
            "tools/call",
            json!({
                "name": "classify_inventory",
                "arguments": {
                    "inventory": {
                        "schema": "mcp-recon.inventory.v1",
                        "servers": [{
                            "name": "t",
                            "tools": [{
                                "name": "execute_shell",
                                "description": "Execute a shell command.",
                                "side_effects": [],
                                "auth_required": true
                            }]
                        }]
                    }
                }
            }),
        ))
        .expect("response");
        assert!(
            resp.get("error").is_none(),
            "expected success, got error: {resp}"
        );
        let content = &resp["result"]["content"];
        assert_eq!(content[0]["type"], "text");
        let text = content[0]["text"].as_str().expect("text");
        let findings: Value = serde_json::from_str(text).expect("findings are valid JSON");
        let arr = findings.as_array().expect("findings is an array");
        assert!(!arr.is_empty(), "should fire at least R7 on execute_shell");
        let has_r7 = arr
            .iter()
            .any(|f| f["id"].as_str().unwrap_or("").contains("r7"));
        assert!(has_r7, "R7 should fire on shell execution surface");
    }

    #[test]
    fn tools_call_caveats_returns_caveat_artifact_text() {
        let resp = dispatch(req(
            "tools/call",
            json!({
                "name": "caveats",
                "arguments": {
                    "inventory": {
                        "schema": "mcp-recon.inventory.v1",
                        "servers": [{
                            "name": "t",
                            "tools": [{
                                "name": "order.refund",
                                "description": "Refund an order",
                                "parameters": {
                                    "type": "object",
                                    "properties": { "amount": { "type": "number" } }
                                },
                                "side_effects": ["write","money"],
                                "auth_required": true
                            }]
                        }]
                    }
                }
            }),
        ))
        .expect("response");
        assert!(resp.get("error").is_none(), "got error: {resp}");
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        let art: Value = serde_json::from_str(text).expect("caveats artifact JSON");
        assert_eq!(art["schema"], "mcp-recon/v0.1/caveats");
        let plans = art["plans"].as_array().unwrap();
        assert!(!plans.is_empty());
        assert!(plans.iter().any(|p| p["caveats"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str().unwrap_or("").contains("arg.amount"))));
    }

    #[test]
    fn tools_call_missing_arguments_yields_invalid_params() {
        let resp =
            dispatch(req("tools/call", json!({ "name": "classify_inventory" }))).expect("response");
        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn tools_call_unknown_tool_yields_method_not_found() {
        // Tool-name validation runs BEFORE inventory parsing, so a typo'd
        // tool name with garbage arguments still surfaces as METHOD_NOT_FOUND
        // rather than confusing the caller with an INVALID_PARAMS message
        // about an inventory they never meant to provide.
        let resp = dispatch(req(
            "tools/call",
            json!({ "name": "wat", "arguments": { "inventory": {} } }),
        ))
        .expect("response");
        assert_eq!(resp["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn tools_call_malformed_inventory_yields_invalid_params() {
        let resp = dispatch(req(
            "tools/call",
            json!({
                "name": "classify_inventory",
                "arguments": { "inventory": { "schema": "wrong", "servers": "not-an-array" } }
            }),
        ))
        .expect("response");
        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
    }
}
