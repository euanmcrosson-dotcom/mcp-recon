//! End-to-end integration tests for `mcp-recon adapt`.
//!
//! Drives the real binary against the committed Anthropic + OpenAI fixtures
//! under `examples/`, then asserts both the emitted inventory shape AND that
//! the classifier produces the expected findings on the adapted output —
//! covering the whole adapter→classifier pipeline a user would actually run.

use std::path::{Path, PathBuf};
use std::process::Command;

use mcp_recon_core::{classify, McpInventory};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn temp_path(stem: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{stem}-{stamp}.json"))
}

fn run_adapt(format: &str, input: &Path, output: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_mcp-recon"))
        .args([
            "adapt",
            "--format",
            format,
            input.to_string_lossy().as_ref(),
            "--out",
            output.to_string_lossy().as_ref(),
            "--pretty",
        ])
        .status()
        .expect("spawn mcp-recon adapt");
    assert!(status.success(), "mcp-recon adapt failed for {format}");
}

fn load_inventory(path: &Path) -> McpInventory {
    let body = std::fs::read_to_string(path).expect("read output");
    serde_json::from_str(&body).expect("parse mcp-recon.inventory.v1")
}

#[test]
fn anthropic_fixture_adapts_and_fires_expected_rules() {
    let input = manifest_dir().join("../../examples/anthropic-tools.json");
    let output = temp_path("mcp-recon-adapt-anthropic");

    run_adapt("anthropic", &input, &output);
    let inv = load_inventory(&output);
    let _ = std::fs::remove_file(&output);

    // Shape: one server with four tools, names preserved.
    assert_eq!(inv.servers.len(), 1);
    let names: Vec<&str> = inv.servers[0]
        .tools
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "get_order_status",
            "send_shipping_notification",
            "refund_order",
            "execute_python",
        ]
    );

    // End-to-end: classifier runs on the adapted output and surfaces the
    // rules the fixture is designed to trip.
    let findings = classify(&inv);
    let rule_ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();

    // R7 on execute_python (description says "Execute arbitrary Python code").
    assert!(
        rule_ids.iter().any(|id| id.contains("r7") && id.contains("execute_python")),
        "R7 should fire on execute_python; got {rule_ids:?}"
    );
    // R3 on send_shipping_notification (name has "send", side_effects undeclared).
    assert!(
        rule_ids
            .iter()
            .any(|id| id.contains("r3") && id.contains("send_shipping_notification")),
        "R3 should fire on send_shipping_notification; got {rule_ids:?}"
    );
    // R4 on refund_order.amount (unbounded numeric on a money-shaped name).
    assert!(
        rule_ids.iter().any(|id| id.contains("r4") && id.contains("refund_order")),
        "R4 should fire on refund_order; got {rule_ids:?}"
    );
}

#[test]
fn openai_fixture_adapts_and_fires_expected_rules() {
    let input = manifest_dir().join("../../examples/openai-tools.json");
    let output = temp_path("mcp-recon-adapt-openai");

    run_adapt("openai", &input, &output);
    let inv = load_inventory(&output);
    let _ = std::fs::remove_file(&output);

    let names: Vec<&str> = inv.servers[0]
        .tools
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["get_weather", "fetch_url", "charge_customer", "run_shell"]
    );

    let findings = classify(&inv);
    let rule_ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();

    // R7 on run_shell (description "Run a shell command").
    assert!(
        rule_ids.iter().any(|id| id.contains("r7") && id.contains("run_shell")),
        "R7 should fire on run_shell; got {rule_ids:?}"
    );
    // R6 on fetch_url (description "Fetch the URL and return its contents").
    assert!(
        rule_ids.iter().any(|id| id.contains("r6") && id.contains("fetch_url")),
        "R6 should fire on fetch_url; got {rule_ids:?}"
    );
    // R4 on charge_customer.amount (unbounded money numeric).
    assert!(
        rule_ids.iter().any(|id| id.contains("r4") && id.contains("charge_customer")),
        "R4 should fire on charge_customer; got {rule_ids:?}"
    );
    // get_weather should be clean — no findings against it.
    assert!(
        !rule_ids.iter().any(|id| id.contains("get_weather")),
        "get_weather should produce no findings; got {rule_ids:?}"
    );
}

#[test]
fn adapt_server_name_override_propagates_to_inventory() {
    let input = manifest_dir().join("../../examples/anthropic-tools.json");
    let output = temp_path("mcp-recon-adapt-named");

    let status = Command::new(env!("CARGO_BIN_EXE_mcp-recon"))
        .args([
            "adapt",
            "--format",
            "anthropic",
            input.to_string_lossy().as_ref(),
            "--out",
            output.to_string_lossy().as_ref(),
            "--server-name",
            "shopify-bot",
        ])
        .status()
        .expect("spawn");
    assert!(status.success());
    let inv = load_inventory(&output);
    let _ = std::fs::remove_file(&output);
    assert_eq!(inv.servers[0].name, "shopify-bot");
}

#[test]
fn adapt_default_server_name_is_the_input_file_stem() {
    let input = manifest_dir().join("../../examples/anthropic-tools.json");
    let output = temp_path("mcp-recon-adapt-default-name");
    run_adapt("anthropic", &input, &output);
    let inv = load_inventory(&output);
    let _ = std::fs::remove_file(&output);
    assert_eq!(inv.servers[0].name, "anthropic-tools");
}
