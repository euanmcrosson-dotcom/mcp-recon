//! End-to-end integration test for `mcp-recon enumerate`.
//!
//! Spins up the Python mock MCP server (tests/fixtures/mock_mcp_server.py) via
//! a generated claude_desktop_config.json, runs the real `mcp-recon enumerate`
//! binary against it, and asserts the emitted inventory contains the mock's
//! two tools. Skips gracefully if no Python interpreter is on PATH.

use std::path::PathBuf;
use std::process::Command;

/// First Python interpreter on PATH that runs, or None.
fn python_cmd() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|cand| {
        Command::new(cand)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

#[test]
fn enumerate_mock_server_yields_inventory_with_two_tools() {
    let Some(py) = python_cmd() else {
        eprintln!("skipping: no python interpreter on PATH");
        return;
    };

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mock = manifest.join("tests/fixtures/mock_mcp_server.py");
    assert!(
        mock.exists(),
        "mock server fixture missing: {}",
        mock.display()
    );

    // Build a claude_desktop_config.json pointing at the mock, in a temp dir.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!("mcp-recon-enum-test-{stamp}"));
    std::fs::create_dir_all(&tmp).unwrap();
    let config_path = tmp.join("config.json");
    let inv_path = tmp.join("inventory.json");

    let config = serde_json::json!({
        "mcpServers": {
            "mock": { "command": py, "args": [mock.to_string_lossy()] }
        }
    });
    std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();

    // Run the real binary: mcp-recon enumerate <config> --out <inv> --pretty
    let status = Command::new(env!("CARGO_BIN_EXE_mcp-recon"))
        .args([
            "enumerate",
            config_path.to_string_lossy().as_ref(),
            "--out",
            inv_path.to_string_lossy().as_ref(),
            "--pretty",
            "--timeout-secs",
            "20",
        ])
        .status()
        .expect("run mcp-recon enumerate");
    assert!(status.success(), "enumerate exited non-zero");

    // Inspect the inventory.
    let body = std::fs::read_to_string(&inv_path).expect("inventory written");
    let inv: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(inv["schema"], "mcp-recon.inventory.v1");
    let servers = inv["servers"].as_array().expect("servers array");
    assert_eq!(servers.len(), 1, "one configured server");
    let tools = servers[0]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 2, "mock exposes two tools; got {tools:?}");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"execute_shell_command"));
    assert!(names.contains(&"read_file"));
    // inputSchema should have mapped into parameters
    assert!(tools[0]["parameters"].is_object() || tools[1]["parameters"].is_object());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn enumerate_then_classify_flags_the_shell_tool_critical() {
    // The whole point: enumerate a live server, then classify the inventory and
    // confirm R7 escalates the shell-exec tool to critical.
    let Some(py) = python_cmd() else {
        eprintln!("skipping: no python interpreter on PATH");
        return;
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mock = manifest.join("tests/fixtures/mock_mcp_server.py");

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!("mcp-recon-pipe-test-{stamp}"));
    std::fs::create_dir_all(&tmp).unwrap();
    let config_path = tmp.join("config.json");
    let inv_path = tmp.join("inventory.json");
    let findings_path = tmp.join("findings.json");

    std::fs::write(
        &config_path,
        serde_json::to_string(&serde_json::json!({
            "mcpServers": { "mock": { "command": py, "args": [mock.to_string_lossy()] } }
        }))
        .unwrap(),
    )
    .unwrap();

    // enumerate
    assert!(Command::new(env!("CARGO_BIN_EXE_mcp-recon"))
        .args([
            "enumerate",
            config_path.to_string_lossy().as_ref(),
            "--out",
            inv_path.to_string_lossy().as_ref()
        ])
        .status()
        .unwrap()
        .success());

    // classify
    assert!(Command::new(env!("CARGO_BIN_EXE_mcp-recon"))
        .args([
            "--target",
            inv_path.to_string_lossy().as_ref(),
            "--out",
            findings_path.to_string_lossy().as_ref()
        ])
        .status()
        .unwrap()
        .success());

    let findings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&findings_path).unwrap()).unwrap();
    let critical: Vec<&str> = findings["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["severity"] == "critical")
        .filter_map(|f| f["tool"].as_str())
        .collect();
    assert!(
        critical.contains(&"execute_shell_command"),
        "R7 should flag the shell tool critical; findings: {}",
        serde_json::to_string_pretty(&findings).unwrap()
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
