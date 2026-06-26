//! Integration tests against the canned example inventories under
//! `mcp-recon/examples/`. Asserts the rule output is stable so HN demos
//! can rely on specific numbers.

use mcp_recon_core::{caveats_v1, classify, McpInventory};

fn load(name: &str) -> McpInventory {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

#[test]
fn shopify_inventory_produces_expected_findings() {
    let inv = load("shopify-mcp.inventory.json");
    let findings = classify(&inv);
    let by_rule: std::collections::BTreeMap<&str, usize> =
        findings.iter().fold(Default::default(), |mut acc, f| {
            let rule = f.id.split('-').nth(1).unwrap_or("?");
            *acc.entry(rule).or_insert(0) += 1;
            acc
        });
    assert_eq!(
        findings.len(),
        7,
        "expected 7 findings; got {} ({:?})\nfull dump:\n{}",
        findings.len(),
        by_rule,
        serde_json::to_string_pretty(&findings).unwrap()
    );
    // Spot-check rule distribution.
    assert!(
        by_rule.get("r1").copied().unwrap_or(0) >= 2,
        "R1 should fire >=2 times"
    );
    assert_eq!(
        by_rule.get("r3").copied().unwrap_or(0),
        1,
        "R3 once on fulfill.send_tracking"
    );
    assert_eq!(
        by_rule.get("r4").copied().unwrap_or(0),
        1,
        "R4 once on order.refund.amount"
    );
    assert_eq!(
        by_rule.get("r5").copied().unwrap_or(0),
        1,
        "R5 once on order.refund"
    );
    assert_eq!(
        by_rule.get("r6").copied().unwrap_or(0),
        1,
        "R6 once on product.summarize_competitor"
    );
    assert_eq!(
        by_rule.get("r8").copied().unwrap_or(0),
        1,
        "R8 once on product.summarize_competitor (unconstrained URL param)"
    );
}

#[test]
fn dvmcp_inventory_escalates_execution_tools_to_critical() {
    // Damn Vulnerable MCP Server (real tool surface, faithful to the repo).
    // Every tool has an unconstrained string input (R1, medium); the two
    // code/command-execution tools must escalate to critical via R7.
    let inv = load("dvmcp.inventory.json");
    let findings = classify(&inv);
    assert_eq!(
        findings.len(),
        12,
        "expected 12 findings; got {}",
        findings.len()
    );
    let critical: Vec<&str> = findings
        .iter()
        .filter(|f| matches!(f.severity, mcp_recon_core::Severity::Critical))
        .filter_map(|f| f.tool.as_deref())
        .collect();
    assert_eq!(
        critical.len(),
        2,
        "expected 2 critical findings; got {critical:?}"
    );
    assert!(critical.contains(&"execute_python_code"));
    assert!(critical.contains(&"execute_shell_command"));
}

#[test]
fn safe_inventory_produces_zero_findings() {
    let inv = load("safe-mcp.inventory.json");
    let findings = classify(&inv);
    assert!(
        findings.is_empty(),
        "safe-mcp should be clean; got:\n{}",
        serde_json::to_string_pretty(&findings).unwrap()
    );

    // …and a clean inventory yields no issuance plans either.
    assert!(caveats_v1(&inv).plans.is_empty());
}

#[test]
fn dvmcp_caveats_deny_the_two_execution_tools() {
    // The Find → Bind handoff: the two RCE tools must become `deny` plans.
    let inv = load("dvmcp.inventory.json");
    let art = caveats_v1(&inv);
    assert_eq!(art.schema, "mcp-recon/v0.1/caveats");
    let denied: Vec<&str> = art
        .plans
        .iter()
        .filter(|p| p.recommend == "deny")
        .map(|p| p.tool.as_str())
        .collect();
    assert!(
        denied.contains(&"execute_python_code") && denied.contains(&"execute_shell_command"),
        "both exec tools should be deny plans; got {denied:?}"
    );
    // Each deny plan carries a `tool != "…"` exclusion caveat.
    for p in art.plans.iter().filter(|p| p.recommend == "deny") {
        assert!(
            p.caveats.iter().any(|c| c.starts_with("tool != ")),
            "deny plan for {} should exclude the tool; got {:?}",
            p.tool,
            p.caveats
        );
    }
}
