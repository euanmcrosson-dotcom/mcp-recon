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

/// Serialize a tool name as a capnagent caveat-DSL string literal.
///
/// The DSL grammar (capnagent-core `caveat_dsl.rs` §grammar) accepts exactly
/// four escape sequences inside a `"..."` string: `\n`, `\t`, `\\`, `\"`.
/// Anything else after a backslash is a parse error on the receiving end —
/// so emitting predicates with Rust's `Debug` formatter (which uses a larger
/// escape set: `\r`, `\0`, `\u{..}`) silently produces caveats that capnagent
/// cannot parse, breaking the Find→Bind handoff.
///
/// Returns `Ok(literal)` (with surrounding quotes) for any name whose chars
/// are representable in the DSL — non-ASCII passes through unescaped, since
/// the DSL parser does not restrict it. Returns `Err` for any control char
/// other than newline or tab (including NUL, CR, DEL, and the C0/C1 ranges),
/// which would require an escape the DSL does not support.
fn dsl_string_literal(s: &str) -> Result<String, ()> {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c.is_control()) => return Err(()),
            c => out.push(c),
        }
    }
    out.push('"');
    Ok(out)
}

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
        if !rules
            .iter()
            .any(|r| ADDRESSABLE_RULES.contains(&r.as_str()))
        {
            continue; // e.g. R1-only: input hygiene, not an authority decision.
        }
        let provenance: Vec<String> = rules.iter().cloned().collect();

        // Serialize the tool name as a DSL string literal. If the name carries
        // a control character the DSL can't escape (anything outside the four
        // legal escapes), we fail closed: emit a `deny` plan with no caveats
        // and a note explaining why. An empty `caveats` list on a `deny` plan
        // is the schema-level signal that the issuer must refuse to bind any
        // capability that could include this tool.
        let Ok(name_lit) = dsl_string_literal(tool) else {
            plans.push(CaveatPlan {
                tool: tool.clone(),
                recommend: "deny".into(),
                caveats: Vec::new(),
                provenance,
                note: "Tool name contains a control character outside the caveat DSL's \
                       legal escape set (only \\n, \\t, \\\\, \\\" are permitted). The \
                       name cannot be safely embedded in a `tool == \"…\"` predicate; \
                       capnagent's issuer must refuse to bind any capability covering \
                       this tool until the upstream server renames it."
                    .into(),
            });
            continue;
        };

        let plan = if rules.contains("r7") {
            CaveatPlan {
                tool: tool.clone(),
                recommend: "deny".into(),
                caveats: vec![format!("tool != {name_lit}")],
                provenance,
                note: "Code/command-execution surface (R7): arbitrary execution \
                       collapses every other caveat. Do not grant this tool; the \
                       caveat excludes it from any broader capability."
                    .into(),
            }
        } else {
            let mut caveats = vec![format!("tool == {name_lit}")];
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
        assert!(plan
            .caveats
            .contains(&r#"tool != "puppeteer_evaluate""#.to_string()));
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
        assert!(plan
            .caveats
            .contains(&r#"tool == "order.refund""#.to_string()));
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
        assert!(
            art.plans.is_empty(),
            "no findings → no plans; got {:?}",
            art.plans
        );
    }

    #[test]
    fn artifact_carries_schema_tag() {
        let art = caveats_v1(&inv(vec![]));
        assert_eq!(art.schema, "mcp-recon/v0.1/caveats");
    }

    // ── DSL interop ────────────────────────────────────────────────────────
    //
    // Every caveat string we emit has to round-trip through capnagent's
    // `parse_string`. capnagent only knows four escapes (\n, \t, \\, \").
    // We vendor a byte-for-byte equivalent of that string-literal parser
    // here so the test asserts the exact contract the consumer enforces,
    // without taking a dep on capnagent-core.

    /// Mirrors `capnagent-core::caveat_dsl::Parser::parse_string`. Returns the
    /// decoded payload on success, or an error string on parse failure.
    fn capnagent_parse_string(input: &str) -> Result<String, String> {
        let mut chars = input.chars();
        match chars.next() {
            Some('"') => {}
            _ => return Err("expected opening '\"'".into()),
        }
        let mut out = String::new();
        loop {
            match chars.next() {
                Some('"') => {
                    if chars.next().is_some() {
                        return Err("trailing chars after closing '\"'".into());
                    }
                    return Ok(out);
                }
                Some('\\') => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(c) => return Err(format!("invalid escape '\\{c}'")),
                    None => return Err("trailing backslash".into()),
                },
                Some(c) => out.push(c),
                None => return Err("unterminated string".into()),
            }
        }
    }

    /// Pull every `tool == "…"` / `tool != "…"` literal out of a caveat string
    /// and re-parse the literal portion through the capnagent-equivalent
    /// parser. Returns the decoded tool name(s), or the first parse error.
    fn tool_literal_round_trip(caveat: &str) -> Result<String, String> {
        let lit_start = caveat
            .find('"')
            .ok_or_else(|| "no string literal in caveat".to_string())?;
        capnagent_parse_string(&caveat[lit_start..])
    }

    #[test]
    fn dsl_string_literal_passes_normal_names_through() {
        let lit = dsl_string_literal("order.refund").expect("safe name");
        assert_eq!(lit, "\"order.refund\"");
        assert_eq!(capnagent_parse_string(&lit).unwrap(), "order.refund");
    }

    #[test]
    fn dsl_string_literal_escapes_quote_and_backslash() {
        // A pathological-but-legal name containing characters that BOTH need
        // escaping and ARE supported by the capnagent grammar.
        let name = r#"weird"name\with"chars"#;
        let lit = dsl_string_literal(name).expect("safe name (quotes/backslash are escapable)");
        // capnagent must decode the literal back to the original bytes.
        assert_eq!(capnagent_parse_string(&lit).unwrap(), name);
    }

    #[test]
    fn dsl_string_literal_handles_newline_and_tab() {
        let name = "line1\nline2\ttab";
        let lit = dsl_string_literal(name).expect("\\n and \\t are in the DSL escape set");
        assert_eq!(capnagent_parse_string(&lit).unwrap(), name);
    }

    #[test]
    fn dsl_string_literal_rejects_carriage_return() {
        // CR is the canonical case: Rust's `Debug` emits literal `\r`, which
        // capnagent's parser rejects as an invalid escape. We must fail closed
        // instead of emitting a caveat the consumer can't parse.
        assert!(dsl_string_literal("foo\rbar").is_err());
    }

    #[test]
    fn dsl_string_literal_rejects_null_and_other_controls() {
        for bad in ["foo\0bar", "foo\x07bar", "foo\x1bbar", "foo\x7fbar"] {
            assert!(
                dsl_string_literal(bad).is_err(),
                "control-char name {bad:?} must fail closed, not emit unparseable caveat"
            );
        }
    }

    #[test]
    fn dsl_string_literal_passes_non_ascii_through() {
        // capnagent's parser does not restrict non-control non-ASCII chars.
        for ok in ["café", "日本語", "𝐀rbitrary-unicode"] {
            let lit = dsl_string_literal(ok).expect("non-ASCII is DSL-safe");
            assert_eq!(capnagent_parse_string(&lit).unwrap(), ok);
        }
    }

    #[test]
    fn caveats_artifact_round_trips_through_capnagent_grammar() {
        // Names that exercise the escape set the DSL DOES support.
        let names = [
            "order.refund",
            "weird\"name",
            "back\\slash",
            "line1\nline2",
            "tab\there",
            "café-tool",
            "日本-search",
        ];
        for name in names {
            let art = caveats_v1(&inv(vec![tool(
                name,
                "Refund an order",
                json!({ "type": "object", "properties": { "amount": { "type": "number" } } }),
            )]));
            let plan = art
                .plans
                .iter()
                .find(|p| p.tool == name)
                .unwrap_or_else(|| panic!("expected a plan for {name:?}; got {:?}", art.plans));
            assert_eq!(plan.recommend, "scope", "scope plan for {name:?}");
            // The first caveat is always `tool == "<name>"`. Round-trip the
            // literal portion through the capnagent grammar and confirm we
            // get the original name back.
            let first = plan.caveats.first().expect("at least one caveat");
            assert!(
                first.starts_with("tool == "),
                "first caveat should be a tool== predicate; got {first:?}"
            );
            let decoded = tool_literal_round_trip(first).expect("capnagent must parse our caveat");
            assert_eq!(
                decoded, name,
                "round-trip mismatch: caveat {first:?} decoded to {decoded:?}, expected {name:?}"
            );
        }
    }

    #[test]
    fn r7_caveat_round_trips_through_capnagent_grammar() {
        // R7 emits `tool != "<name>"`. Same round-trip requirement.
        let art = caveats_v1(&inv(vec![tool(
            r#"weird"exec\name"#,
            "Execute arbitrary code",
            json!(null),
        )]));
        let plan = art
            .plans
            .iter()
            .find(|p| p.tool == r#"weird"exec\name"#)
            .expect("plan for R7 tool");
        assert_eq!(plan.recommend, "deny");
        let first = plan.caveats.first().expect("R7 plan has one caveat");
        assert!(first.starts_with("tool != "));
        let decoded = tool_literal_round_trip(first).expect("capnagent must parse the deny caveat");
        assert_eq!(decoded, r#"weird"exec\name"#);
    }

    #[test]
    fn tool_name_with_control_char_fails_closed_to_empty_deny_plan() {
        // A tool name containing CR (or any other control char outside \n/\t)
        // cannot be embedded in a DSL literal. Rather than emit an unparseable
        // caveat, the plan must fail closed: recommend=deny, caveats=[], with
        // an explanatory note. Any R-rule that flagged the tool is still
        // captured in `provenance` so the human reader sees why.
        let art = caveats_v1(&inv(vec![tool(
            "evil\rrefund",
            "Refund an order",
            json!({ "type": "object", "properties": { "amount": { "type": "number" } } }),
        )]));
        let plan = art
            .plans
            .iter()
            .find(|p| p.tool == "evil\rrefund")
            .expect("plan for the unsafe-name tool");
        assert_eq!(
            plan.recommend, "deny",
            "fail-closed should recommend deny when the name can't be serialized"
        );
        assert!(
            plan.caveats.is_empty(),
            "no caveat string should be emitted for an unsafe name; got {:?}",
            plan.caveats
        );
        assert!(
            plan.note.contains("control character"),
            "note should explain the fail-closed reason; got {:?}",
            plan.note
        );
        // Provenance is preserved so the human reader sees which rules fired.
        assert!(
            plan.provenance.contains(&"r4".to_string()),
            "provenance should still record the rules that fired; got {:?}",
            plan.provenance
        );
    }

    #[test]
    fn tool_name_with_null_byte_fails_closed() {
        let art = caveats_v1(&inv(vec![tool(
            "puppeteer\0evaluate",
            "Execute JavaScript",
            json!(null),
        )]));
        let plan = art
            .plans
            .iter()
            .find(|p| p.tool == "puppeteer\0evaluate")
            .expect("plan for the unsafe-name R7 tool");
        assert_eq!(plan.recommend, "deny");
        assert!(plan.caveats.is_empty());
        // Provenance should still include r7 even though we couldn't emit
        // the `tool != "…"` caveat.
        assert!(plan.provenance.contains(&"r7".to_string()));
    }
}
