//! Live MCP enumeration (stdio + HTTP transports).
//!
//! Reads a `claude_desktop_config.json` (the same shape Cursor and Cline use),
//! reaches each configured MCP server — either by launching it as a stdio
//! subprocess or by POSTing to its HTTP endpoint — performs the MCP
//! `initialize` handshake, calls `tools/list`, and builds an
//! `mcp-recon.inventory.v1` from the discovered tools. The resulting inventory
//! can then be fed to the classifier (`mcp-recon --target …`).
//!
//! A server entry is HTTP if it has a `url` field, stdio if it has a `command`.
//! HTTP uses the MCP Streamable HTTP transport: each JSON-RPC message is POSTed
//! and the reply arrives as either `application/json` or a `text/event-stream`
//! SSE body; the `Mcp-Session-Id` response header is echoed on later requests.

use anyhow::{anyhow, Context, Result};
use mcp_recon_core::{McpInventory, McpServer, Tool, Transport, INVENTORY_SCHEMA};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// MCP protocol version we advertise in the handshake.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Map an MCP `tools/list` result value (`{ "tools": [ {name, description,
/// inputSchema}, … ] }`) into inventory tools. MCP doesn't self-declare
/// side-effects / auth, so those are left empty — the classifier's
/// declaration-gated rules (R2/R3/R5) honestly won't fire on enumerated tools,
/// while the schema/name/description rules (R1/R4/R6/R7) will.
fn tools_from_list_result(result: &serde_json::Value) -> Vec<Tool> {
    let Some(arr) = result.get("tools").and_then(|t| t.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_string();
            Some(Tool {
                name,
                description: t
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(String::from),
                parameters: t.get("inputSchema").cloned(),
                side_effects: Vec::new(),
                auth_required: None,
                rate_limited: None,
            })
        })
        .collect()
}

/// A `claude_desktop_config.json` — only the fields we consume.
#[derive(Debug, Deserialize)]
struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: BTreeMap<String, RawSpec>,
}

/// The raw per-server config object, before we decide stdio vs HTTP.
#[derive(Debug, Deserialize)]
struct RawSpec {
    /// stdio: executable to run (e.g. "npx").
    command: Option<String>,
    /// stdio: arguments passed to the command.
    #[serde(default)]
    args: Vec<String>,
    /// stdio: extra environment variables for the child process.
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// HTTP: the server endpoint URL.
    url: Option<String>,
    /// HTTP: extra request headers (e.g. an `Authorization` bearer).
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

/// How to reach one MCP server.
#[derive(Debug, Clone)]
pub enum Launch {
    /// Launch a local subprocess and speak JSON-RPC over its stdio.
    Stdio {
        /// Executable to run.
        command: String,
        /// Arguments passed to the command.
        args: Vec<String>,
        /// Extra environment variables for the child process.
        env: BTreeMap<String, String>,
    },
    /// POST JSON-RPC to a remote HTTP endpoint (Streamable HTTP transport).
    Http {
        /// The server endpoint URL.
        url: String,
        /// Extra request headers (e.g. an `Authorization` bearer).
        headers: BTreeMap<String, String>,
    },
}

/// A configured server with its logical name and how to reach it.
#[derive(Debug, Clone)]
pub struct NamedServer {
    /// The key under `mcpServers`.
    pub name: String,
    /// How to reach the server.
    pub launch: Launch,
}

/// Parse a `claude_desktop_config.json` into a name-sorted list of servers.
/// An entry with a `url` is HTTP; an entry with a `command` is stdio.
pub fn parse_config(json: &str) -> Result<Vec<NamedServer>> {
    let cfg: ClaudeDesktopConfig =
        serde_json::from_str(json).context("parse claude_desktop_config.json")?;
    // BTreeMap iteration is already name-sorted, giving deterministic output.
    let mut out = Vec::with_capacity(cfg.mcp_servers.len());
    for (name, raw) in cfg.mcp_servers {
        let launch = if let Some(url) = raw.url {
            Launch::Http {
                url,
                headers: raw.headers,
            }
        } else if let Some(command) = raw.command {
            Launch::Stdio {
                command,
                args: raw.args,
                env: raw.env,
            }
        } else {
            return Err(anyhow!(
                "server `{name}` has neither a `command` (stdio) nor a `url` (http)"
            ));
        };
        out.push(NamedServer { name, launch });
    }
    Ok(out)
}

/// Enumerate every server in a `claude_desktop_config.json` and assemble an
/// `mcp-recon.inventory.v1`. A server that fails to connect (not an MCP server,
/// times out, errors) becomes an entry with an empty tool list rather than
/// aborting the whole run.
pub fn enumerate_config(json: &str, timeout: Duration) -> Result<McpInventory> {
    let servers = parse_config(json)?;
    let mut out = Vec::with_capacity(servers.len());
    for ns in servers {
        let (transport, result) = match &ns.launch {
            Launch::Stdio { command, args, env } => (
                Transport::Stdio,
                list_tools_stdio(command, args, env, timeout),
            ),
            Launch::Http { url, headers } => {
                (Transport::Http, list_tools_http(url, headers, timeout))
            }
        };
        let tools = match result {
            Ok(t) => {
                eprintln!("  ✓ {} — {} tools", ns.name, t.len());
                t
            }
            Err(e) => {
                eprintln!("  ✗ {} — {e:#}", ns.name);
                Vec::new()
            }
        };
        out.push(McpServer {
            name: ns.name,
            transport: Some(transport),
            tools,
        });
    }
    Ok(McpInventory {
        schema: INVENTORY_SCHEMA.into(),
        servers: out,
    })
}

/// Spawn one stdio MCP server, perform the `initialize` handshake, call
/// `tools/list`, and map the result to inventory tools.
fn list_tools_stdio(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Vec<Tool>> {
    // Resolve the command through PATH/PATHEXT so Windows launcher shims
    // (npx.cmd, uvx.cmd, …) are found — `Command::new("npx")` alone only
    // resolves .exe on Windows. Fall back to the raw name if resolution fails.
    let program = which::which(command).unwrap_or_else(|_| std::path::PathBuf::from(command));
    let mut child = Command::new(&program)
        .args(args)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn `{command}`"))?;

    let mut stdin = child.stdin.take().context("open child stdin")?;
    let stdout = child.stdout.take().context("open child stdout")?;

    // Reader thread → channel of lines, so the main thread can recv_timeout.
    let (tx, rx) = mpsc::channel::<String>();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let result = (|| -> Result<Vec<Tool>> {
        write_msg(
            &mut stdin,
            &serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "mcp-recon", "version": env!("CARGO_PKG_VERSION") }
                }
            }),
        )?;
        recv_response(&rx, 1, timeout).context("initialize handshake")?;

        write_msg(
            &mut stdin,
            &serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )?;

        write_msg(
            &mut stdin,
            &serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
        )?;
        let resp = recv_response(&rx, 2, timeout).context("tools/list")?;
        if let Some(err) = resp.get("error") {
            return Err(anyhow!("server returned error for tools/list: {err}"));
        }
        let result = resp.get("result").cloned().unwrap_or_default();
        Ok(tools_from_list_result(&result))
    })();

    // Cleanup regardless of outcome.
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    drop(rx);
    let _ = reader.join();

    result
}

/// Perform the MCP handshake over Streamable HTTP and return the tools.
fn list_tools_http(
    url: &str,
    headers: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Vec<Tool>> {
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let mut session: Option<String> = None;

    http_rpc(
        &agent,
        url,
        headers,
        &mut session,
        &serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "mcp-recon", "version": env!("CARGO_PKG_VERSION") }
            }
        }),
        Some(1),
    )
    .context("initialize handshake")?
    .ok_or_else(|| anyhow!("no initialize response from {url} — not an MCP HTTP endpoint?"))?;

    http_rpc(
        &agent,
        url,
        headers,
        &mut session,
        &serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        None,
    )
    .context("notifications/initialized")?;

    let resp = http_rpc(
        &agent,
        url,
        headers,
        &mut session,
        &serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
        Some(2),
    )
    .context("tools/list")?
    .ok_or_else(|| anyhow!("no tools/list response from {url}"))?;

    if let Some(err) = resp.get("error") {
        return Err(anyhow!("server returned error for tools/list: {err}"));
    }
    let result = resp.get("result").cloned().unwrap_or_default();
    Ok(tools_from_list_result(&result))
}

/// POST one JSON-RPC message and, when `want_id` is set, return the response
/// object with that id. Carries the `Mcp-Session-Id` across calls.
fn http_rpc(
    agent: &ureq::Agent,
    url: &str,
    headers: &BTreeMap<String, String>,
    session: &mut Option<String>,
    msg: &serde_json::Value,
    want_id: Option<i64>,
) -> Result<Option<serde_json::Value>> {
    let mut req = agent
        .post(url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream");
    for (k, v) in headers {
        req = req.set(k, v);
    }
    if let Some(sid) = session.as_deref() {
        req = req.set("Mcp-Session-Id", sid);
    }
    let resp = req
        .send_string(&serde_json::to_string(msg)?)
        .map_err(|e| anyhow!("HTTP request to {url} failed: {e}"))?;

    if let Some(sid) = resp.header("mcp-session-id") {
        *session = Some(sid.to_string());
    }
    let content_type = resp.header("content-type").unwrap_or("").to_string();
    let body = resp.into_string().context("read HTTP response body")?;

    let Some(id) = want_id else {
        return Ok(None);
    };
    Ok(parse_http_body(&content_type, &body)
        .into_iter()
        .find(|m| m.get("id").and_then(serde_json::Value::as_i64) == Some(id)))
}

/// Parse an HTTP MCP response body into JSON-RPC messages, handling both a
/// plain `application/json` object and a `text/event-stream` (SSE) body.
fn parse_http_body(content_type: &str, body: &str) -> Vec<serde_json::Value> {
    if content_type.contains("text/event-stream") {
        sse_json_messages(body)
    } else {
        serde_json::from_str::<serde_json::Value>(body)
            .map(|v| vec![v])
            .unwrap_or_default()
    }
}

/// Extract JSON objects from the `data:` fields of an SSE body. Multiple
/// `data:` lines within one event are joined with newlines (per the SSE spec)
/// before being parsed; non-JSON events are skipped.
fn sse_json_messages(body: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut data = String::new();
    let flush = |data: &mut String, out: &mut Vec<serde_json::Value>| {
        if !data.is_empty() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                out.push(v);
            }
            data.clear();
        }
    };
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        } else if line.trim().is_empty() {
            flush(&mut data, &mut out);
        }
    }
    flush(&mut data, &mut out);
    out
}

/// Write one newline-delimited JSON-RPC message to the child's stdin.
fn write_msg(stdin: &mut std::process::ChildStdin, v: &serde_json::Value) -> Result<()> {
    let s = serde_json::to_string(v)?;
    stdin.write_all(s.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

/// Read lines until a JSON-RPC response with `id` arrives (skipping
/// notifications, logs, and non-JSON noise), or time out / disconnect.
fn recv_response(
    rx: &mpsc::Receiver<String>,
    id: i64,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| anyhow!("timed out waiting for response id={id}"))?;
        let line = match rx.recv_timeout(remaining) {
            Ok(l) => l,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(anyhow!("timed out waiting for response id={id}"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(
                    "server closed stdout before responding (id={id}) — not an MCP server?"
                ))
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue; // some servers print non-JSON banners to stdout; skip
        };
        if v.get("id").and_then(serde_json::Value::as_i64) == Some(id) {
            return Ok(v);
        }
        // otherwise a notification / different id — keep reading
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "mcpServers": {
            "filesystem": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
            },
            "remote": {
                "url": "https://mcp.example.com/mcp",
                "headers": { "Authorization": "Bearer xyz" }
            }
        }
    }"#;

    #[test]
    fn parses_stdio_and_http_servers_sorted_by_name() {
        let servers = parse_config(SAMPLE).unwrap();
        assert_eq!(servers.len(), 2);
        // BTreeMap → sorted: filesystem before remote
        assert_eq!(servers[0].name, "filesystem");
        match &servers[0].launch {
            Launch::Stdio { command, args, .. } => {
                assert_eq!(command, "npx");
                assert_eq!(args[0], "-y");
            }
            other => panic!("expected stdio, got {other:?}"),
        }
        assert_eq!(servers[1].name, "remote");
        match &servers[1].launch {
            Launch::Http { url, headers } => {
                assert_eq!(url, "https://mcp.example.com/mcp");
                assert_eq!(headers.get("Authorization").map(String::as_str), Some("Bearer xyz"));
            }
            other => panic!("expected http, got {other:?}"),
        }
    }

    #[test]
    fn rejects_server_with_neither_command_nor_url() {
        let json = r#"{ "mcpServers": { "bad": { "args": ["x"] } } }"#;
        assert!(parse_config(json).is_err());
    }

    #[test]
    fn empty_config_yields_no_servers() {
        assert_eq!(parse_config("{}").unwrap().len(), 0);
    }

    #[test]
    fn sse_body_yields_jsonrpc_messages() {
        // One SSE event carrying a JSON-RPC response, plus noise lines.
        let body = "event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\
                    \n";
        let msgs = sse_json_messages(body);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"].as_i64(), Some(2));
    }

    #[test]
    fn sse_body_joins_multiline_data() {
        let body = "data: {\"jsonrpc\":\"2.0\",\n\
                    data: \"id\":7}\n\
                    \n";
        let msgs = sse_json_messages(body);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"].as_i64(), Some(7));
    }

    #[test]
    fn parse_http_body_handles_plain_json() {
        let msgs = parse_http_body(
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        );
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"].as_i64(), Some(1));
    }

    #[test]
    fn maps_tools_list_result_to_inventory_tools() {
        let result = serde_json::json!({
            "tools": [
                {
                    "name": "read_file",
                    "description": "Read a file from disk",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                },
                { "name": "ping", "description": "no params" }
            ]
        });
        let tools = tools_from_list_result(&result);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(
            tools[0].description.as_deref(),
            Some("Read a file from disk")
        );
        assert!(tools[0].parameters.is_some(), "inputSchema → parameters");
        assert!(
            tools[0].side_effects.is_empty(),
            "MCP doesn't declare side-effects"
        );
        assert_eq!(tools[1].name, "ping");
        assert!(tools[1].parameters.is_none());
    }

    #[test]
    fn maps_empty_or_missing_tools_to_nothing() {
        assert_eq!(tools_from_list_result(&serde_json::json!({})).len(), 0);
        assert_eq!(
            tools_from_list_result(&serde_json::json!({"tools": []})).len(),
            0
        );
    }

    #[test]
    fn malformed_json_errors() {
        assert!(parse_config("not json").is_err());
    }
}
