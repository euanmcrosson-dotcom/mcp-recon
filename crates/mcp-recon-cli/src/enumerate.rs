//! Live MCP enumeration (stdio transport).
//!
//! Reads a `claude_desktop_config.json` (the same shape Cursor and Cline use),
//! launches each configured MCP server as a subprocess, performs the MCP
//! `initialize` handshake, calls `tools/list`, and builds an
//! `mcp-recon.inventory.v1` from the discovered tools. The resulting inventory
//! can then be fed to the classifier (`mcp-recon --target …`).
//!
//! v1 supports stdio transport only — the transport every local MCP server
//! (Claude Desktop / Cursor / Cline) uses. HTTP/SSE is a follow-up.

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
    mcp_servers: BTreeMap<String, ServerLaunchSpec>,
}

/// How to launch one MCP server (stdio).
#[derive(Debug, Clone, Deserialize)]
pub struct ServerLaunchSpec {
    /// Executable to run (e.g. "npx").
    pub command: String,
    /// Arguments passed to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables for the child process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// A configured server with its logical name.
#[derive(Debug, Clone)]
pub struct NamedServer {
    /// The key under `mcpServers`.
    pub name: String,
    /// Launch spec.
    pub spec: ServerLaunchSpec,
}

/// Parse a `claude_desktop_config.json` into a name-sorted list of servers.
pub fn parse_config(json: &str) -> Result<Vec<NamedServer>> {
    let cfg: ClaudeDesktopConfig =
        serde_json::from_str(json).context("parse claude_desktop_config.json")?;
    // BTreeMap iteration is already name-sorted, giving deterministic output.
    Ok(cfg
        .mcp_servers
        .into_iter()
        .map(|(name, spec)| NamedServer { name, spec })
        .collect())
}

/// Enumerate every server in a `claude_desktop_config.json` and assemble an
/// `mcp-recon.inventory.v1`. A server that fails to connect (not an MCP server,
/// times out, errors) becomes an entry with an empty tool list rather than
/// aborting the whole run.
pub fn enumerate_config(json: &str, timeout: Duration) -> Result<McpInventory> {
    let servers = parse_config(json)?;
    let mut out = Vec::with_capacity(servers.len());
    for ns in servers {
        let tools = match list_tools_stdio(&ns.spec, timeout) {
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
            transport: Some(Transport::Stdio),
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
fn list_tools_stdio(spec: &ServerLaunchSpec, timeout: Duration) -> Result<Vec<Tool>> {
    let mut child = Command::new(&spec.command)
        .args(&spec.args)
        .envs(&spec.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn `{}`", spec.command))?;

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
            "shopify": {
                "command": "node",
                "args": ["server.js"],
                "env": { "SHOPIFY_API_KEY": "xxx" }
            }
        }
    }"#;

    #[test]
    fn parses_two_servers_sorted_by_name() {
        let servers = parse_config(SAMPLE).unwrap();
        assert_eq!(servers.len(), 2);
        // BTreeMap → sorted: filesystem before shopify
        assert_eq!(servers[0].name, "filesystem");
        assert_eq!(servers[0].spec.command, "npx");
        assert_eq!(
            servers[0].spec.args,
            vec!["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
        );
        assert_eq!(servers[1].name, "shopify");
        assert_eq!(servers[1].spec.command, "node");
        assert_eq!(
            servers[1]
                .spec
                .env
                .get("SHOPIFY_API_KEY")
                .map(String::as_str),
            Some("xxx")
        );
    }

    #[test]
    fn empty_config_yields_no_servers() {
        assert_eq!(parse_config("{}").unwrap().len(), 0);
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
