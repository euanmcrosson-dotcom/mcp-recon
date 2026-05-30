//! Live HTTP MCP producer — third producer path for the Capframe
//! leaderboard.
//!
//! Treats the corpus URL as an MCP server endpoint speaking the
//! "Streamable HTTP" transport from the MCP spec. The handshake is
//! the same shape as stdio:
//!
//!   POST initialize → response carries `Mcp-Session-Id`
//!   POST notifications/initialized
//!   POST tools/list → response carries the live tool surface
//!
//! Server responses can come back as either `application/json`
//! (single response) or `text/event-stream` (an SSE stream where
//! the first `message` event carries the JSON-RPC response). We
//! handle both — the SSE parser is intentionally minimal and bails
//! the moment it sees the matching JSON-RPC reply, so we don't sit
//! waiting on the connection.
//!
//! Authentication is intentionally NOT implemented here: this path
//! is for public/unauthenticated endpoints only. Real bearer tokens
//! belong in deployment-side secrets, not in a public corpus file.

use anyhow::{anyhow, bail, Context, Result};
use mcp_recon_core::{McpServer, SideEffect, Tool, Transport};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::time::Duration;

const HTTP_TIMEOUT_SECS: u64 = 30;
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Fetch live `tools/list` from an HTTP MCP endpoint and synthesise
/// an McpServer for the classifier. Errors propagate; the caller
/// (`run_registry`) logs and moves on so one bad endpoint doesn't
/// tank the corpus walk.
pub fn fetch_server(url: &str) -> Result<McpServer> {
    let mut client = HttpClient::new(url);
    let tools = client
        .handshake_and_list_tools()
        .with_context(|| format!("HTTP MCP handshake for {url}"))?;
    Ok(McpServer {
        name: url.to_string(),
        transport: Some(Transport::Http),
        tools,
    })
}

struct HttpClient {
    endpoint: String,
    session_id: Option<String>,
    next_id: u64,
}

impl HttpClient {
    fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            session_id: None,
            next_id: 0,
        }
    }

    fn handshake_and_list_tools(&mut self) -> Result<Vec<Tool>> {
        // 1. initialize
        let init_id = self.next_id();
        let init_resp = self.rpc(
            init_id,
            "initialize",
            Some(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "capframe-mcp-recon-http",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            })),
        )?;
        init_resp
            .get("result")
            .ok_or_else(|| anyhow!("initialize returned no result: {init_resp}"))?;

        // 2. notifications/initialized (no response expected)
        self.notification("notifications/initialized")?;

        // 3. tools/list
        let tools_id = self.next_id();
        let resp = self.rpc(tools_id, "tools/list", None)?;
        let tools_arr = resp
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("tools/list response missing result.tools: {resp}"))?;
        normalise_tools(tools_arr)
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn rpc(&mut self, id: u64, method: &str, params: Option<Value>) -> Result<Value> {
        let mut body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(p) = params {
            body["params"] = p;
        }
        self.post(&body, /*expect_response*/ true)?
            .ok_or_else(|| anyhow!("RPC {method} returned no body"))
    }

    fn notification(&mut self, method: &str) -> Result<()> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        let _ = self.post(&body, /*expect_response*/ false)?;
        Ok(())
    }

    /// POST a JSON-RPC message; if `expect_response` is true, parse the
    /// reply (either JSON or SSE) and return it.
    fn post(&mut self, body: &Value, expect_response: bool) -> Result<Option<Value>> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build();
        let mut req = agent
            .post(&self.endpoint)
            .set("Content-Type", "application/json")
            // Streamable HTTP servers may respond with either, so we
            // declare we accept both.
            .set("Accept", "application/json, text/event-stream")
            .set(
                "User-Agent",
                concat!("mcp-recon/", env!("CARGO_PKG_VERSION")),
            )
            .set("MCP-Protocol-Version", PROTOCOL_VERSION);
        if let Some(sid) = &self.session_id {
            req = req.set("Mcp-Session-Id", sid);
        }

        let payload = serde_json::to_string(body).context("serialise RPC body")?;
        let res = req.send_string(&payload);
        let resp = match res {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let snippet = r.into_string().unwrap_or_default();
                let trimmed: String = snippet.chars().take(200).collect();
                bail!("HTTP {code}: {trimmed}");
            }
            Err(e) => bail!("HTTP transport error: {e}"),
        };

        // Capture session id if the server returned one.
        if let Some(sid) = resp.header("Mcp-Session-Id") {
            self.session_id = Some(sid.to_string());
        }

        if !expect_response {
            // Drain the body so the connection can pool, but ignore it.
            let _ = resp.into_string();
            return Ok(None);
        }

        let content_type = resp
            .header("Content-Type")
            .unwrap_or("application/json")
            .to_ascii_lowercase();
        if content_type.starts_with("text/event-stream") {
            let id = body.get("id").cloned().unwrap_or(Value::Null);
            let parsed = parse_sse_response(resp.into_reader(), &id)?;
            Ok(Some(parsed))
        } else {
            let text = resp.into_string().context("read HTTP body")?;
            let v: Value = serde_json::from_str(&text)
                .with_context(|| format!("parse JSON response body: {}", truncate(&text, 200)))?;
            Ok(Some(v))
        }
    }
}

/// Parse a Server-Sent Events response and return the first JSON-RPC
/// message whose `id` matches `want_id`. Bails out as soon as a match
/// is found so we don't sit on the stream waiting for further events.
fn parse_sse_response<R: std::io::Read>(reader: R, want_id: &Value) -> Result<Value> {
    let buf = BufReader::new(reader);
    let mut data_buf = String::new();
    for line in buf.lines() {
        let line = line.context("read SSE line")?;
        if line.is_empty() {
            // Event boundary — flush.
            if !data_buf.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(data_buf.trim()) {
                    if v.get("id") == Some(want_id) {
                        return Ok(v);
                    }
                }
                data_buf.clear();
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            // SSE allows multiple `data:` lines per event; concatenate.
            if !data_buf.is_empty() {
                data_buf.push('\n');
            }
            data_buf.push_str(rest.trim_start());
        }
        // Other fields (event:, id:, retry:) — ignore for our purpose.
    }
    // EOF without empty-line terminator; try the last buffered event.
    if !data_buf.is_empty() {
        if let Ok(v) = serde_json::from_str::<Value>(data_buf.trim()) {
            if v.get("id") == Some(want_id) {
                return Ok(v);
            }
        }
    }
    Err(anyhow!("SSE stream ended without a matching response"))
}

fn normalise_tools(tools: &[Value]) -> Result<Vec<Tool>> {
    let mut out = Vec::with_capacity(tools.len());
    for t in tools {
        let name = t
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("tool entry missing name"))?
            .to_string();
        let description = t
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string);
        let parameters = t.get("inputSchema").cloned();
        let side_effects = infer_side_effects(&name);
        out.push(Tool {
            name,
            description,
            parameters,
            side_effects,
            auth_required: None,
            rate_limited: None,
        });
    }
    Ok(out)
}

fn infer_side_effects(name: &str) -> Vec<SideEffect> {
    let n = name.to_lowercase();
    if n.contains("exec_") || n.starts_with("exec ") || n == "exec" || n.contains("run_command") {
        vec![SideEffect::Execute]
    } else {
        vec![]
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_message_with_matching_id() {
        let sse = "event: message\n\
                   data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n";
        let v = parse_sse_response(sse.as_bytes(), &json!(2)).unwrap();
        assert_eq!(v["result"]["tools"], json!([]));
    }

    #[test]
    fn parses_sse_with_multiple_data_lines() {
        let sse = "data: {\"jsonrpc\":\"2.0\",\n\
                   data: \"id\":7,\n\
                   data: \"result\":{\"tools\":[{\"name\":\"x\"}]}}\n\n";
        let v = parse_sse_response(sse.as_bytes(), &json!(7)).unwrap();
        assert_eq!(v["result"]["tools"][0]["name"], "x");
    }

    #[test]
    fn skips_sse_messages_whose_id_doesnt_match() {
        let sse = "data: {\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{}}\n\
                   \n\
                   data: {\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{\"tools\":[]}}\n\
                   \n";
        let v = parse_sse_response(sse.as_bytes(), &json!(4)).unwrap();
        assert_eq!(v["result"]["tools"], json!([]));
    }

    #[test]
    fn sse_with_no_matching_id_errors() {
        let sse = "data: {\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{}}\n\n";
        let err = parse_sse_response(sse.as_bytes(), &json!(4)).unwrap_err();
        assert!(err.to_string().contains("SSE stream ended"));
    }

    #[test]
    fn normalises_tools_keeps_input_schema() {
        let tools = vec![json!({
            "name": "fetch_url",
            "description": "fetch a URL",
            "inputSchema": { "type": "object", "properties": { "u": { "type": "string" } } }
        })];
        let out = normalise_tools(&tools).unwrap();
        assert_eq!(out[0].name, "fetch_url");
        assert!(out[0].parameters.is_some());
    }

    #[test]
    fn normalises_tools_errors_on_missing_name() {
        let tools = vec![json!({ "description": "no name" })];
        assert!(normalise_tools(&tools).is_err());
    }

    #[test]
    fn infers_execute_only_on_obvious_names() {
        assert!(matches!(
            infer_side_effects("exec_shell")[..],
            [SideEffect::Execute]
        ));
        assert!(infer_side_effects("read_file").is_empty());
    }
}
