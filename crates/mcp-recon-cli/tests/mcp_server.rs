//! End-to-end integration test for `mcp-recon mcp-server`.
//!
//! Spawns the real binary, talks newline-delimited JSON-RPC over stdio,
//! and asserts each MCP protocol turn — `initialize`, `tools/list`, and
//! `tools/call` for both tools — returns the shape the spec requires.
//!
//! This is the test that an MCP client (Claude Desktop, Cursor, an agent
//! framework) would implicitly run when it connects; failing it means the
//! server doesn't speak the protocol correctly.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Spawn the server, drive it with the given JSON-RPC requests (one per
/// line), and collect the response lines. The server is fed all requests
/// then has its stdin closed; it should exit on EOF and we collect every
/// response line before then.
fn drive_server(requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-recon"))
        .arg("mcp-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-recon mcp-server");

    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        for req in requests {
            writeln!(stdin, "{}", serde_json::to_string(req).unwrap())
                .expect("write request");
        }
        stdin.flush().expect("flush");
    }
    // Drop stdin to signal EOF so the server exits.
    drop(child.stdin.take());

    let stdout = child.stdout.take().expect("child stdout");
    let reader = BufReader::new(stdout);
    let mut responses = Vec::new();
    for line in reader.lines() {
        let line = line.expect("read line");
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("non-JSON response {line:?}: {e}"));
        responses.push(v);
    }

    // Give the child a moment to exit; kill if it doesn't.
    let _ = child.wait_timeout_or_kill(Duration::from_secs(5));
    responses
}

trait WaitTimeoutOrKill {
    fn wait_timeout_or_kill(&mut self, timeout: Duration) -> std::io::Result<()>;
}

impl WaitTimeoutOrKill for std::process::Child {
    fn wait_timeout_or_kill(&mut self, timeout: Duration) -> std::io::Result<()> {
        // Cheap manual poll — avoids a dep on `wait-timeout`. After we drop
        // stdin the server should exit promptly; this loop is just a guard
        // against a regression that leaves stdin-EOF unhandled.
        let start = std::time::Instant::now();
        loop {
            match self.try_wait()? {
                Some(_) => return Ok(()),
                None => {
                    if start.elapsed() >= timeout {
                        let _ = self.kill();
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }
}

#[test]
fn server_handles_full_protocol_handshake_and_tool_call() {
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "integration-test", "version": "0.0.0" }
        }
    });
    let initialized_notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let tools_list = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let classify_call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "classify_inventory",
            "arguments": {
                "inventory": {
                    "schema": "mcp-recon.inventory.v1",
                    "servers": [{
                        "name": "test",
                        "tools": [{
                            "name": "execute_shell_command",
                            "description": "Execute a shell command.",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "cmd": { "type": "string", "maxLength": 4096 }
                                }
                            },
                            "side_effects": [],
                            "auth_required": true
                        }]
                    }]
                }
            }
        }
    });
    let caveats_call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "caveats",
            "arguments": {
                "inventory": {
                    "schema": "mcp-recon.inventory.v1",
                    "servers": [{
                        "name": "test",
                        "tools": [{
                            "name": "order.refund",
                            "description": "Refund an order",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "amount": { "type": "number" }
                                }
                            },
                            "side_effects": ["write","money"],
                            "auth_required": true
                        }]
                    }]
                }
            }
        }
    });

    let responses = drive_server(&[
        initialize,
        initialized_notification,
        tools_list,
        classify_call,
        caveats_call,
    ]);

    // We sent 4 requests + 1 notification. The notification gets no
    // response, so we expect exactly 4 responses.
    assert_eq!(
        responses.len(),
        4,
        "expected 4 responses (notification yields none); got {}: {:#?}",
        responses.len(),
        responses
    );

    // initialize → protocolVersion + serverInfo
    let init = &responses[0];
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(init["result"]["serverInfo"]["name"], "mcp-recon");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    // tools/list → both tools advertised
    let list = &responses[1];
    assert_eq!(list["id"], 2);
    let tool_names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(tool_names.contains(&"classify_inventory"));
    assert!(tool_names.contains(&"caveats"));

    // tools/call classify_inventory → text content with at least one R7 finding
    let classify_resp = &responses[2];
    assert_eq!(classify_resp["id"], 3);
    assert!(
        classify_resp.get("error").is_none(),
        "classify_inventory errored: {classify_resp:#?}"
    );
    let text = classify_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let findings: serde_json::Value = serde_json::from_str(text).expect("findings JSON");
    let arr = findings.as_array().expect("findings array");
    assert!(
        arr.iter()
            .any(|f| f["id"].as_str().unwrap_or("").contains("r7")),
        "R7 should fire on execute_shell_command; got {arr:#?}"
    );

    // tools/call caveats → arg.amount <= 100 caveat for refund
    let caveats_resp = &responses[3];
    assert_eq!(caveats_resp["id"], 4);
    assert!(caveats_resp.get("error").is_none(), "caveats errored: {caveats_resp:#?}");
    let cav_text = caveats_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("caveats text");
    let cav_artifact: serde_json::Value =
        serde_json::from_str(cav_text).expect("caveats JSON");
    assert_eq!(cav_artifact["schema"], "mcp-recon/v0.1/caveats");
    let plans = cav_artifact["plans"].as_array().expect("plans array");
    let has_amount_cap = plans.iter().any(|p| {
        p["caveats"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str().unwrap_or("").contains("arg.amount"))
    });
    assert!(
        has_amount_cap,
        "expected an arg.amount cap for the refund tool; got {plans:#?}"
    );
}

#[test]
fn server_returns_parse_error_for_garbage_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-recon"))
        .arg("mcp-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");

    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, "this is not JSON-RPC").unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());

    let stdout = child.stdout.take().unwrap();
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line).unwrap();

    let _ = child.wait_timeout_or_kill(Duration::from_secs(5));

    let v: serde_json::Value = serde_json::from_str(line.trim()).expect("response is JSON");
    // Parse-error responses carry a null id (we couldn't parse one).
    assert_eq!(v["jsonrpc"], "2.0");
    assert!(v["id"].is_null());
    assert_eq!(v["error"]["code"], -32700);
}
