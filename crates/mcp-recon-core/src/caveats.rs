//! Find → Bind handoff: turn classifier findings into capnagent-ready issuance
//! plans.
//!
//! [`caveats_v1`] classifies an inventory and, for every tool that carries an
//! *authority-relevant* finding (R2/R3/R4/R6/R7 — not input-hygiene R1), emits
//! a [`CaveatPlan`]: a recommendation (`deny` for code-execution surfaces,
//! otherwise `scope`) plus capnagent caveat-DSL strings and the rule provenance
//! that drove them. The whole artifact is `mcp-recon/v0.1/caveats`, the shape
//! capnagent's issuer consumes.
//!
//! Caveat DSL emitted (a subset of capnagent's grammar):
//! - `tool != "name"` — exclude a tool from a capability (deny).
//! - `tool == "name"` — scope a capability to a single tool (allow).
//! - `arg.<param> <= 100` — cap an unbounded money/quota numeric (placeholder
//!   limit; callers set their real policy value).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::classifier::{classify, unbounded_money_params};
use crate::inventory::{McpInventory, Tool};

/// Schema tag for the caveats artifact (consumed by capnagent's issuer).
pub const CAVEATS_SCHEMA: &str = "mcp-recon/v0.1/caveats";

/// Rules that imply an authority decision (and so a caveat). R1 (unconstrained
/// string) and R5 (money-in-description) are deliberately excluded: the former
/// is input hygiene, the latter is a declaration hint, not a scoping target.
const ADDRESSABLE_RULES: &[&str] = &["r2", "r3", "r4", "r6", "r7"];

/// One tool's recommended capability issuance plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaveatPlan {
    /// The tool this plan governs.
    pub tool: String,
    /// `"deny"` (do not grant — code execution) or `"scope"` (allow, bounded).
    pub recommend: String,
    /// capnagent caveat-DSL strings to apply.
    pub caveats: Vec<String>,
    /// Rule ids (sorted) that produced this plan, for traceability.
    pub provenance: Vec<String>,
    /// Human-readable rationale.
    pub note: String,
}

/// The full `mcp-recon/v0.1/caveats` artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaveatArtifact {
    /// Schema tag (`mcp-recon/v0.1/caveats`).
    pub schema: String,
    /// One plan per authority-relevant tool, ordered by tool name.
    pub plans: Vec<CaveatPlan>,
}

/// Classify `inv` and build the caveats artifact.
pub fn caveats_v1(inv: &McpInventory) -> CaveatArtifact {
    let findings = classify(inv);

    // tool name → its schema (first occurrence wins), for arg-level caveats.
    let tools: BTreeMap<&str, &Tool> = inv
        .servers
        .iter()
        .flat_map(|s| &s.tools)
        .map(|t| (t.name.as_str(), t))
        .collect();

    // tool name → set of rule ids that fired on it.
    let mut rules_by_tool: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in &findings {
        let Some(tool) = f.tool.as_deref() else {
            continue;
        };
        let Some(rule) = f.id.strip_prefix("f-").and_then(|s| s.split('-').next()) else {
            continue;
        };
        rules_by_tool
            .entry(tool.to_string())
            .or_default()
            .insert(rule.to_string());
    }

    let mut plans = Vec::new();
    for (tool, rules) in &rules_by_tool {
        if !rules.iter().any(|r| ADDRESSABLE_RULES.contains(&r.as_str())) {
            continue; // e.g. R1-only: input hygiene, not an authority decision.
        }
        let provenance: Vec<String> = rules.iter().cloned().collect();

        let plan = if rules.contains("r7") {
            CaveatPlan {
                tool: tool.clone(),
                recommend: "deny".into(),
                caveats: vec![format!("tool != {tool:?}")],
                provenance,
                note: "Code/command-execution surface (R7): arbitrary execution \
                       collapses every other caveat. Do not grant this tool; the \
                       caveat excludes it from any broader capability."
                    .into(),
            }
        } else {
            let mut caveats = vec![format!("tool == {tool:?}")];
            let mut money = false;
            if rules.contains("r4") {
                if let Some(t) = tools.get(tool.as_str()) {
                    for p in unbounded_money_params(t) {
                        caveats.push(format!("arg.{p} <= 100"));
                        money = true;
                    }
                }
            }
            let note = if money {
                "Scope a capability to this tool and cap the money/quota argument. \
                 The `<= 100` limit is a placeholder — set it to your real policy value."
                    .to_string()
            } else {
                "Scope a capability to this tool; deny anything outside it.".to_string()
            };
            CaveatPlan {
                tool: tool.clone(),
                recommend: "scope".into(),
                caveats,
                provenance,
                note,
            }
        };
        plans.push(plan);
    }

    CaveatArtifact {
        schema: CAVEATS_SCHEMA.into(),
        plans,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{McpServer, SideEffect, Transport};
    use serde_json::json;

    fn inv(tools: Vec<Tool>) -> McpInventory {
        McpInventory {
            schema: crate::inventory::INVENTORY_SCHEMA.into(),
            servers: vec![McpServer {
                name: "s".into(),
                transport: Some(Transport::Stdio),
                tools,
            }],
        }
    }

    fn tool(name: &str, desc: &str, params: serde_json::Value) -> Tool {
        Tool {
            name: name.into(),
            description: Some(desc.into()),
            parameters: if params.is_null() { None } else { Some(params) },
            side_effects: vec![],
            auth_required: Some(true),
            rate_limited: None,
        }
    }

    #[test]
    fn r7_tool_gets_a_deny_plan() {
        let art = caveats_v1(&inv(vec![tool(
            "puppeteer_evaluate",
            "Execute JavaScript in the browser console",
            json!(null),
        )]));
        let plan = art
            .plans
            .iter()
            .find(|p| p.tool == "puppeteer_evaluate")
            .expect("a plan for the exec tool");
        assert_eq!(plan.recommend, "deny");
        assert!(plan.caveats.contains(&r#"tool != "puppeteer_evaluate""#.to_string()));
        assert!(plan.provenance.contains(&"r7".to_string()));
    }

    #[test]
    fn money_tool_gets_scope_plan_with_arg_cap() {
        let art = caveats_v1(&inv(vec![tool(
            "order.refund",
            "Refund an order",
            json!({ "type": "object", "properties": { "amount": { "type": "number" } } }),
        )]));
        let plan = art
            .plans
            .iter()
            .find(|p| p.tool == "order.refund")
            .expect("a plan for the refund tool");
        assert_eq!(plan.recommend, "scope");
        assert!(plan.caveats.contains(&r#"tool == "order.refund""#.to_string()));
        assert!(
            plan.caveats.contains(&"arg.amount <= 100".to_string()),
            "expected an amount cap; got {:?}",
            plan.caveats
        );
        assert!(plan.provenance.contains(&"r4".to_string()));
    }

    #[test]
    fn r1_only_tool_gets_no_plan() {
        // Unconstrained string is input hygiene, not an authority decision.
        let art = caveats_v1(&inv(vec![Tool {
            name: "lookup".into(),
            description: Some("Look something up".into()),
            parameters: Some(json!({
                "type": "object", "properties": { "q": { "type": "string" } }
            })),
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        }]));
        assert!(
            art.plans.iter().all(|p| p.tool != "lookup"),
            "R1-only tool should not get a caveat plan; got {:?}",
            art.plans
        );
    }

    #[test]
    fn clean_tool_yields_no_plans() {
        let art = caveats_v1(&inv(vec![Tool {
            name: "get_status".into(),
            description: Some("Return service status".into()),
            parameters: Some(json!({
                "type": "object", "properties": { "id": { "type": "string", "maxLength": 32 } }
            })),
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        }]));
        assert!(art.plans.is_empty(), "no findings → no plans; got {:?}", art.plans);
    }

    #[test]
    fn artifact_carries_schema_tag() {
        let art = caveats_v1(&inv(vec![]));
        assert_eq!(art.schema, "mcp-recon/v0.1/caveats");
    }
}
