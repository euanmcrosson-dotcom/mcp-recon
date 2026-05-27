//! Rule-based classifier.
//!
//! Takes an [`McpInventory`] snapshot and emits a list of [`Finding`]s.
//! Rules are deterministic -- the same inventory produces the same findings
//! every run. Each rule maps to a category + a small bag of compliance
//! framework IDs (OWASP LLM Top 10, NIST AI RMF, MITRE ATLAS).

use serde::{Deserialize, Serialize};

use crate::findings::{Category, Finding, Mappings, Severity};
use crate::inventory::{McpInventory, SideEffect, Tool};

/// Top-level data-class assignment. Retained from v0.1 scaffold for backward
/// compatibility with the [`Classification`] type; not emitted in findings
/// directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    /// Filesystem read / write / delete.
    Filesystem,
    /// HTTP / fetch / network egress.
    Network,
    /// Shell / exec / spawn.
    Shell,
    /// Money movement / refunds / charges.
    Payments,
    /// Email / chat / notifications / paging.
    Messaging,
    /// System metadata (process info, env vars, machine identity).
    System,
    /// Read-only metadata about the world (clock, weather, lookups).
    Metadata,
    /// Unknown / does not match any rule.
    Unknown,
}

/// Heuristic authority level -- what the tool can do once invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityLevel {
    /// Read-only access within a bounded scope.
    Read,
    /// Mutates state within a bounded scope (writes, sends, charges).
    Write,
    /// Removes state irreversibly (delete, drop, cancel).
    Destructive,
    /// Spawns subprocesses, opens shells, or otherwise hands authority to a child.
    Privileged,
}

/// Intermediate classification entry per tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    /// Tool name from the inventory.
    pub tool: String,
    /// Assigned data-class.
    pub data_class: DataClass,
    /// Assigned authority level.
    pub authority_level: AuthorityLevel,
    /// Whether the tool is flagged as a confused-deputy candidate.
    pub confused_deputy_candidate: bool,
    /// Free-form rationale string for human review.
    pub rationale: String,
}

/// Run every rule against `inventory` and return the resulting findings.
pub fn classify(inventory: &McpInventory) -> Vec<Finding> {
    let mut out = Vec::new();
    for server in &inventory.servers {
        for tool in &server.tools {
            out.extend(rule_r1_unconstrained_string(tool));
            out.extend(rule_r2_missing_auth(tool));
            out.extend(rule_r3_side_effect_mismatch(tool));
            out.extend(rule_r4_unbounded_money_param(tool));
            out.extend(rule_r5_description_money_no_side_effect(tool));
            out.extend(rule_r6_external_fetch_surface(tool));
            out.extend(rule_r7_code_execution_surface(tool));
        }
    }
    out
}

// ─── Rules ────────────────────────────────────────────────────────────────

/// R1 -- Unconstrained string input.
///
/// Any string property in `tool.parameters.properties` with no `maxLength`
/// constraint is flagged as a prompt-injection / payload-stuffing surface.
fn rule_r1_unconstrained_string(tool: &Tool) -> Option<Finding> {
    let params = tool.parameters.as_ref()?;
    let props = params.get("properties")?.as_object()?;
    let mut offenders: Vec<&str> = Vec::new();
    for (name, schema) in props {
        let ty = schema.get("type").and_then(|v| v.as_str())?;
        if ty != "string" {
            continue;
        }
        if schema.get("maxLength").is_none() {
            offenders.push(name.as_str());
        }
    }
    if offenders.is_empty() {
        return None;
    }
    offenders.sort();
    let pretty = offenders
        .iter()
        .map(|s| format!("`{s}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(Finding {
        id: stable_id("r1", &tool.name),
        severity: Severity::Medium,
        category: Category::UnconstrainedInput,
        title: format!("Tool `{}` accepts unconstrained string input", tool.name),
        description: Some(format!(
            "The following string parameter(s) have no `maxLength` constraint: {pretty}. \
             Unbounded strings let an attacker stuff arbitrary payloads through the tool, \
             including indirect-injection content."
        )),
        tool: Some(tool.name.clone()),
        remediation: Some(
            "Add a `maxLength` to each string property, or constrain with an `enum` or \
             `pattern`. Most legitimate tool inputs fit under a few hundred bytes."
                .into(),
        ),
        mappings: Mappings {
            owasp_llm: vec!["LLM01".into()],
            nist_rmf: vec!["MEASURE-2.3".into()],
            mitre_atlas: vec!["T0051".into()],
        },
    })
}

/// R2 -- Missing auth declaration on a side-effecting tool.
///
/// Tools that mutate state, send money, or otherwise act on the world should
/// declare `auth_required: true`. Read-only tools are exempt.
fn rule_r2_missing_auth(tool: &Tool) -> Option<Finding> {
    let has_side_effect = tool.side_effects.iter().any(|s| {
        matches!(
            s,
            SideEffect::Write
                | SideEffect::Money
                | SideEffect::Irreversible
                | SideEffect::Filesystem
                | SideEffect::Execute
        )
    });
    if !has_side_effect {
        return None;
    }
    if tool.auth_required == Some(true) {
        return None;
    }
    Some(Finding {
        id: stable_id("r2", &tool.name),
        severity: Severity::High,
        category: Category::MissingAuthz,
        title: format!(
            "Tool `{}` has side effects but does not declare auth_required",
            tool.name
        ),
        description: Some(format!(
            "`{}` declares side effects ({:?}) but `auth_required` is not set to true. \
             Side-effecting tools should require explicit authentication so the agent \
             can't invoke them blindly on behalf of an attacker-controlled prompt.",
            tool.name, tool.side_effects
        )),
        tool: Some(tool.name.clone()),
        remediation: Some(
            "Set `auth_required: true` on the tool, and have the MCP server enforce that \
             callers present credentials before this tool is exposed."
                .into(),
        ),
        mappings: Mappings {
            owasp_llm: vec!["LLM07".into()],
            nist_rmf: vec!["MANAGE-1.3".into()],
            mitre_atlas: vec!["T0049".into()],
        },
    })
}

/// Verbs in tool names that imply state mutation. Matched as a `.`-or-`_`-or-
/// start/end-bounded substring of the lowercased tool name.
const MUTATION_VERBS: &[&str] = &[
    "delete", "remove", "drop", "destroy", "refund", "charge", "transfer", "pay", "purchase",
    "buy", "sell", "send", "post", "publish", "dispatch", "email", "create", "update", "edit",
    "modify", "set", "write", "save", "store", "cancel", "revoke", "expire", "approve", "deny",
    "grant",
    // NB: "issue" intentionally excluded -- far more common as the noun (an
    // issue tracker: get_issue / list_issues / search_issues are reads) than the
    // verb. create_issue / update_issue stay caught by their real verbs.
    // "add" is likewise excluded: it reads as arithmetic/compute as often as
    // mutation (`math.add`, `add_numbers`), so it is too ambiguous to flag on.
];

/// Split a tool name into lowercase word tokens, breaking on non-alphanumeric
/// separators (`_`, `-`, `.`, `/`, …) and on lower→upper camelCase boundaries
/// (`createIssue` → `create`, `issue`).
fn tokenize(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && prev_lower && !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            cur.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        } else if !cur.is_empty() {
            tokens.push(std::mem::take(&mut cur));
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// True if a single `token` is `verb`, or `verb` plus a common plural/tense
/// suffix (so `updates` / `created` match), but bounded to the whole token so
/// the noun `issues` does not match a removed verb and `reset` does not match
/// the verb `set`.
fn token_matches_verb(token: &str, verb: &str) -> bool {
    match token.strip_prefix(verb) {
        Some("") => true,
        Some(suffix) => matches!(suffix, "s" | "es" | "ed" | "d" | "ing"),
        None => false,
    }
}

/// True if any *token* of `tool_name` is a mutation verb. Token-bounded rather
/// than a bare substring, so a verb's letters appearing inside an unrelated
/// word (`issue` in `issues`, `set` in `reset`, `pay` in `payment`) no longer
/// trigger a false match.
fn name_implies_mutation(tool_name: &str) -> bool {
    tokenize(tool_name)
        .iter()
        .any(|tok| MUTATION_VERBS.iter().any(|v| token_matches_verb(tok, v)))
}

/// R3 -- Side-effect / name mismatch.
///
/// If the tool's name implies a mutation verb (delete, send, refund, …) but
/// the declared `side_effects` list doesn't include any write-class effect,
/// the tool is misrepresenting what it does -- exactly the LLM08 excessive-
/// agency pattern.
fn rule_r3_side_effect_mismatch(tool: &Tool) -> Option<Finding> {
    if !name_implies_mutation(&tool.name) {
        return None;
    }
    let has_write_effect = tool.side_effects.iter().any(|s| {
        matches!(
            s,
            SideEffect::Write
                | SideEffect::Money
                | SideEffect::Irreversible
                | SideEffect::Filesystem
                | SideEffect::Execute
        )
    });
    if has_write_effect {
        return None;
    }
    Some(Finding {
        id: stable_id("r3", &tool.name),
        severity: Severity::High,
        category: Category::ExcessiveAgency,
        title: format!(
            "Tool `{}` name implies a side effect that is not declared",
            tool.name
        ),
        description: Some(format!(
            "`{}` looks like a side-effecting tool (its name contains a mutation verb), \
             but its `side_effects` declaration is {:?}. A policy synthesizer cannot \
             produce safe rules for this tool because it cannot tell what it actually does.",
            tool.name, tool.side_effects
        )),
        tool: Some(tool.name.clone()),
        remediation: Some(
            "Declare the tool's true side effects explicitly. If the tool is genuinely \
             read-only, rename it to match (e.g. `email.preview` rather than `email.send`)."
                .into(),
        ),
        mappings: Mappings {
            owasp_llm: vec!["LLM08".into()],
            nist_rmf: vec!["MEASURE-2.6".into()],
            mitre_atlas: vec!["T0051".into()],
        },
    })
}

/// Parameter names that strongly imply a monetary or quota-style value.
const MONEY_PARAM_NAMES: &[&str] = &[
    "amount", "price", "cost", "value", "limit", "qty", "quantity", "total", "fee", "charge",
    "refund", "credit", "debit",
    // NB: bare "max" intentionally excluded -- it matched content/pagination
    // caps like `max_length` / `max_results` / `max_tokens` that move no money.
    // Real spend caps (`max_amount`) are still caught via "amount".
];

/// R4 -- Unbounded numeric on a money-ish parameter.
///
/// Any number-typed property whose name hints at a monetary or quota value
/// without a `maximum` constraint is the canonical LLM08 "refund $1B"
/// failure mode.
fn rule_r4_unbounded_money_param(tool: &Tool) -> Option<Finding> {
    let params = tool.parameters.as_ref()?;
    let props = params.get("properties")?.as_object()?;
    let mut offenders: Vec<&str> = Vec::new();
    for (name, schema) in props {
        let ty = schema.get("type").and_then(|v| v.as_str())?;
        if ty != "number" && ty != "integer" {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if !MONEY_PARAM_NAMES.iter().any(|m| lower.contains(m)) {
            continue;
        }
        if schema.get("maximum").is_none() {
            offenders.push(name.as_str());
        }
    }
    if offenders.is_empty() {
        return None;
    }
    offenders.sort();
    let pretty = offenders
        .iter()
        .map(|s| format!("`{s}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(Finding {
        id: stable_id("r4", &tool.name),
        severity: Severity::High,
        category: Category::ExcessiveAgency,
        title: format!(
            "Tool `{}` accepts an unbounded monetary / quota value",
            tool.name
        ),
        description: Some(format!(
            "The numeric parameter(s) {pretty} have a money/quota-shaped name but no \
             `maximum` constraint. An LLM tricked by indirect-injection can call the \
             tool with arbitrarily large values."
        )),
        tool: Some(tool.name.clone()),
        remediation: Some(
            "Add a `maximum` (and ideally `minimum`) to each money/quota numeric, OR \
             enforce the cap via a capframe-bind `--limit` caveat at the agent boundary."
                .into(),
        ),
        mappings: Mappings {
            owasp_llm: vec!["LLM08".into()],
            nist_rmf: vec!["MANAGE-2.2".into()],
            mitre_atlas: vec!["T0051".into()],
        },
    })
}

/// Words in tool descriptions that strongly imply money movement.
const MONEY_DESC_WORDS: &[&str] = &[
    "payment",
    "pay ",
    "refund",
    "charge",
    "charging",
    "purchase",
    "buy",
    "checkout",
    "invoice",
    "billing",
    "transfer ",
    "wire",
    "payout",
    "topup",
    "top-up",
    "balance",
    // NB: "subscribe"/"subscription" intentionally excluded -- too ambiguous
    // (resource subscriptions, newsletters, event subscriptions all match but
    // aren't money). Found as a false positive scanning the MCP `everything`
    // server's `toggle-subscriber-updates`.
];

/// True if `desc` (case-insensitive) mentions money.
fn description_mentions_money(desc: &str) -> bool {
    let lower = desc.to_ascii_lowercase();
    MONEY_DESC_WORDS.iter().any(|w| lower.contains(w))
}

/// R5 -- Money in description, no money side-effect declared.
///
/// Auditors miss the money operation when a tool's description mentions
/// payment / refund / charge / billing language but the declared
/// `side_effects` list doesn't include `Money`. Same root cause as R3 but a
/// softer signal -- description rather than name.
fn rule_r5_description_money_no_side_effect(tool: &Tool) -> Option<Finding> {
    let desc = tool.description.as_deref()?;
    if !description_mentions_money(desc) {
        return None;
    }
    if tool.side_effects.contains(&SideEffect::Money) {
        return None;
    }
    Some(Finding {
        id: stable_id("r5", &tool.name),
        severity: Severity::Medium,
        category: Category::ExcessiveAgency,
        title: format!(
            "Tool `{}` description mentions money but no `money` side-effect is declared",
            tool.name
        ),
        description: Some(format!(
            "Description: \"{}\" -- this references money/payment/refund/etc., but the \
             declared side_effects ({:?}) don't include `money`. A capframe-bind policy \
             that relies on declared side_effects to scope spend caveats will under-scope \
             this tool.",
            desc, tool.side_effects
        )),
        tool: Some(tool.name.clone()),
        remediation: Some(
            "Add `money` to the tool's `side_effects` declaration, or rewrite the \
             description to clarify that no actual money moves."
                .into(),
        ),
        mappings: Mappings {
            owasp_llm: vec!["LLM08".into()],
            nist_rmf: vec!["MEASURE-2.6".into()],
            mitre_atlas: vec!["T0040".into()],
        },
    })
}

/// Phrases that strongly imply a tool fetches external web content -- the
/// canonical indirect-injection surface.
const FETCH_DESC_PHRASES: &[&str] = &[
    "fetch the",
    "fetch a",
    "fetch url",
    "fetch http",
    "open the url",
    // bounded "download" forms -- bare "download" was a false positive on tools
    // that return *downloadable output* (e.g. a gzip tool) rather than fetching
    // external attacker-controlled content.
    "download the",
    "download a ",
    "download from",
    "download http",
    "scrape",
    "crawl",
    // bounded "browse" forms -- bare "browse" matched the noun "browser",
    // flagging every browser-automation control tool (resize, tabs, screenshot).
    "browse the",
    "browse to",
    "browse http",
    "summarize the",
    "summarize a",
    "read the page",
    "read the url",
    "visit the",
    "load the url",
    "follow the link",
    "follow this link",
];

/// True if `desc` (lowercased) suggests external web fetching.
fn description_implies_external_fetch(desc: &str) -> bool {
    let lower = desc.to_ascii_lowercase();
    FETCH_DESC_PHRASES.iter().any(|p| lower.contains(p))
}

/// R6 -- External-fetch surface in description.
///
/// Tools that pull arbitrary external content into the agent's context window
/// are the canonical LLM01 indirect-injection surface: an attacker-controlled
/// page becomes attacker-controlled prompt input.
fn rule_r6_external_fetch_surface(tool: &Tool) -> Option<Finding> {
    let desc = tool.description.as_deref()?;
    if !description_implies_external_fetch(desc) {
        return None;
    }
    Some(Finding {
        id: stable_id("r6", &tool.name),
        severity: Severity::Medium,
        category: Category::IndirectInjection,
        title: format!(
            "Tool `{}` fetches external web content -- indirect-injection surface",
            tool.name
        ),
        description: Some(format!(
            "Description: \"{desc}\" -- this tool pulls externally-controlled content into the \
             agent's context window, the canonical indirect-injection vector. Even when the \
             user supplies the URL, content at that URL can carry hostile instructions."
        )),
        tool: Some(tool.name.clone()),
        remediation: Some(
            "Sandbox the fetched content: strip prompts before forwarding to the model, \
             constrain to an allow-list of domains, and route through capframe-guard with \
             a `domain in [...]` caveat."
                .into(),
        ),
        mappings: Mappings {
            owasp_llm: vec!["LLM01".into()],
            nist_rmf: vec!["MEASURE-2.3".into()],
            mitre_atlas: vec!["T0051".into()],
        },
    })
}

/// Tokens in a tool's name or description that imply it executes code,
/// shell commands, or arbitrary subprocesses -- the highest-severity surface.
const EXECUTION_TOKENS: &[&str] = &[
    "execute ",
    "execute_",
    "exec ",
    "shell command",
    "shell_command",
    "run python",
    "run code",
    "run a script",
    "run shell",
    "eval(",
    "subprocess",
    "spawn a",
    "arbitrary code",
    "arbitrary command",
    "system(",
];

/// True if name+description (lowercased) implies code/command execution.
fn implies_code_execution(tool: &Tool) -> bool {
    let mut hay = tool.name.to_ascii_lowercase();
    hay.push(' ');
    if let Some(d) = &tool.description {
        hay.push_str(&d.to_ascii_lowercase());
    }
    EXECUTION_TOKENS.iter().any(|t| hay.contains(t))
}

/// R7 -- Code / command execution surface.
///
/// A tool whose name or description implies executing code, shell commands, or
/// subprocesses is the top of the severity ladder: it collapses every other
/// constraint, because arbitrary execution can do anything the host process
/// can. Rated Critical regardless of declared side effects -- the whole point
/// is that these tools are routinely mislabelled (or unlabelled) on real
/// servers, so we don't trust the declaration.
fn rule_r7_code_execution_surface(tool: &Tool) -> Option<Finding> {
    if !implies_code_execution(tool) {
        return None;
    }
    Some(Finding {
        id: stable_id("r7", &tool.name),
        severity: Severity::Critical,
        category: Category::ExcessiveAgency,
        title: format!(
            "Tool `{}` exposes a code/command execution surface",
            tool.name
        ),
        description: Some(format!(
            "`{}` looks like it executes code or shell commands ({}). Arbitrary \
             execution is the maximal authority a tool can hold -- it subsumes every \
             other caveat, so it should never be exposed to an agent without a \
             hard sandbox and an explicit, narrowly-scoped capability.",
            tool.name,
            tool.description.as_deref().unwrap_or("name match")
        )),
        tool: Some(tool.name.clone()),
        remediation: Some(
            "Do not expose raw code/shell execution to an agent. If unavoidable, run \
             it in a disposable sandbox with no network + no host FS, gate it behind a \
             capframe-bind capability scoped to an allow-list of commands, and require \
             holder-of-key proof per call."
                .into(),
        ),
        mappings: Mappings {
            owasp_llm: vec!["LLM08".into()],
            nist_rmf: vec!["MANAGE-2.2".into()],
            mitre_atlas: vec!["T0051".into()],
        },
    })
}

fn stable_id(rule: &str, tool: &str) -> String {
    let mut s = String::with_capacity(rule.len() + tool.len() + 8);
    s.push_str("f-");
    s.push_str(rule);
    s.push('-');
    s.extend(tool.chars().map(|c| {
        if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            '_'
        }
    }));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{McpInventory, McpServer, Transport};
    use serde_json::json;

    fn inventory_with_one_tool(tool: Tool) -> McpInventory {
        McpInventory {
            schema: crate::inventory::INVENTORY_SCHEMA.into(),
            servers: vec![McpServer {
                name: "test-server".into(),
                transport: Some(Transport::Stdio),
                tools: vec![tool],
            }],
        }
    }

    #[test]
    fn r7_fires_critical_on_shell_execution_tool() {
        let tool = Tool {
            name: "execute_shell_command".into(),
            description: Some("Execute a shell command for system management.".into()),
            parameters: Some(json!({
                "type": "object",
                "properties": { "command": { "type": "string", "maxLength": 4096 } }
            })),
            side_effects: vec![],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r7: Vec<_> = findings.iter().filter(|f| f.id.contains("r7")).collect();
        assert_eq!(r7.len(), 1, "expected one R7 finding; got {findings:?}");
        let f = r7[0];
        assert_eq!(f.severity, Severity::Critical);
        assert_eq!(f.category, Category::ExcessiveAgency);
        assert_eq!(f.tool.as_deref(), Some("execute_shell_command"));
        assert_eq!(f.mappings.owasp_llm, vec!["LLM08"]);
    }

    #[test]
    fn r7_fires_on_code_execution_by_description() {
        let tool = Tool {
            name: "analyze".into(),
            description: Some("Execute Python code for data analysis.".into()),
            parameters: None,
            side_effects: vec![],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        assert_eq!(findings.iter().filter(|f| f.id.contains("r7")).count(), 1);
    }

    #[test]
    fn r7_silent_on_benign_tool() {
        let tool = Tool {
            name: "get_user_info".into(),
            description: Some("Get information about a user".into()),
            parameters: None,
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        assert_eq!(findings.iter().filter(|f| f.id.contains("r7")).count(), 0);
    }

    // ── False-positive regressions (surfaced scanning 16 popular MCP servers).
    // All three rules matched a short verb *inside* a longer word: "issue" in
    // "issues"/"get_issue" (R3), "max" in "max_length" (R4), "browse" in
    // "browser" (R6). ──────────────────────────────────────────────────────

    #[test]
    fn r3_silent_on_reads_named_after_the_issue_noun() {
        // "issue" is a noun here (an issue tracker), not the verb "to issue".
        for name in ["list_issues", "get_issue", "search_issues"] {
            let tool = Tool {
                name: name.into(),
                description: Some("Read-only lookup.".into()),
                parameters: None,
                side_effects: vec![],
                auth_required: Some(true),
                rate_limited: None,
            };
            let findings = classify(&inventory_with_one_tool(tool));
            assert_eq!(
                findings.iter().filter(|f| f.id.contains("r3")).count(),
                0,
                "R3 should not fire on read tool `{name}`; got {findings:?}"
            );
        }
    }

    #[test]
    fn r3_still_fires_on_genuine_mutation_verbs() {
        // Regression guard: real mutations stay flagged, including a pluralised
        // verb token (`updates`) and a dotted name (`order.refund`).
        for name in [
            "create_issue",
            "delete_entities",
            "toggle-subscriber-updates",
            "order.refund",
        ] {
            let tool = Tool {
                name: name.into(),
                description: Some("does something".into()),
                parameters: None,
                side_effects: vec![],
                auth_required: Some(true),
                rate_limited: None,
            };
            let findings = classify(&inventory_with_one_tool(tool));
            assert_eq!(
                findings.iter().filter(|f| f.id.contains("r3")).count(),
                1,
                "R3 should fire on mutation tool `{name}`; got {findings:?}"
            );
        }
    }

    #[test]
    fn r4_silent_on_non_money_max_length_param() {
        // `max_length` is a content-length cap, not a money/quota value.
        let tool = Tool {
            name: "fetch".into(),
            description: Some("Return content.".into()),
            parameters: Some(json!({
                "type": "object",
                "properties": { "max_length": { "type": "integer" } }
            })),
            side_effects: vec![],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        assert_eq!(
            findings.iter().filter(|f| f.id.contains("r4")).count(),
            0,
            "R4 should not fire on `max_length`; got {findings:?}"
        );
    }

    #[test]
    fn r4_still_fires_on_unbounded_money_param() {
        let tool = Tool {
            name: "billing".into(),
            description: Some("does something".into()),
            parameters: Some(json!({
                "type": "object",
                "properties": { "amount": { "type": "number" } }
            })),
            side_effects: vec![],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        assert_eq!(
            findings.iter().filter(|f| f.id.contains("r4")).count(),
            1,
            "R4 should fire on unbounded `amount`; got {findings:?}"
        );
    }

    #[test]
    fn r6_silent_on_browser_control_tools() {
        // "browse" must not match the noun "browser". These manipulate browser
        // chrome; they don't pull external content into the context window.
        for (name, desc) in [
            ("browser_resize", "Resize the browser window"),
            ("browser_tabs", "List, close, or select a browser tab."),
        ] {
            let tool = Tool {
                name: name.into(),
                description: Some(desc.into()),
                parameters: None,
                side_effects: vec![],
                auth_required: Some(true),
                rate_limited: None,
            };
            let findings = classify(&inventory_with_one_tool(tool));
            assert_eq!(
                findings.iter().filter(|f| f.id.contains("r6")).count(),
                0,
                "R6 should not fire on browser-control tool `{name}`; got {findings:?}"
            );
        }
    }

    #[test]
    fn r6_still_fires_on_real_external_fetch() {
        for desc in [
            "Fetch the URL and return its contents.",
            "Browse the web for the answer.",
        ] {
            let tool = Tool {
                name: "retrieve".into(),
                description: Some(desc.into()),
                parameters: None,
                side_effects: vec![],
                auth_required: Some(true),
                rate_limited: None,
            };
            let findings = classify(&inventory_with_one_tool(tool));
            assert_eq!(
                findings.iter().filter(|f| f.id.contains("r6")).count(),
                1,
                "R6 should fire on external-fetch desc {desc:?}; got {findings:?}"
            );
        }
    }

    #[test]
    fn r1_fires_on_unconstrained_string_param() {
        // The tool's name and side-effects are crafted so only R1 fires.
        let tool = Tool {
            name: "lookup".into(),
            description: Some("Look something up".into()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "query":  { "type": "string" },
                    "limit":  { "type": "number", "maximum": 100 }
                }
            })),
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r1: Vec<_> = findings
            .iter()
            .filter(|f| f.category == Category::UnconstrainedInput)
            .collect();
        assert_eq!(
            r1.len(),
            1,
            "expected exactly one R1 finding; got {findings:?}"
        );
        let f = r1[0];
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.tool.as_deref(), Some("lookup"));
        assert_eq!(f.mappings.owasp_llm, vec!["LLM01"]);
        assert!(
            f.description.as_deref().unwrap_or("").contains("query"),
            "description should name the offending param"
        );
    }

    #[test]
    fn r1_silent_when_string_has_maxlength() {
        let tool = Tool {
            name: "search".into(),
            description: None,
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "maxLength": 200}
                }
            })),
            side_effects: vec![],
            auth_required: None,
            rate_limited: None,
        };
        assert_eq!(classify(&inventory_with_one_tool(tool)).len(), 0);
    }

    #[test]
    fn r2_fires_when_side_effecting_tool_has_no_auth() {
        let tool = Tool {
            name: "order.refund".into(),
            description: Some("Issue a refund".into()),
            parameters: Some(json!({
                "type": "object",
                "properties": { "order_id": { "type": "string", "maxLength": 64 } }
            })),
            side_effects: vec![SideEffect::Write, SideEffect::Money],
            auth_required: None, // <-- the offence
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r2: Vec<_> = findings
            .iter()
            .filter(|f| f.category == Category::MissingAuthz)
            .collect();
        assert_eq!(r2.len(), 1, "expected exactly one R2 finding");
        let f = r2[0];
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.tool.as_deref(), Some("order.refund"));
        assert_eq!(f.mappings.owasp_llm, vec!["LLM07"]);
    }

    #[test]
    fn r2_silent_when_auth_required_is_true() {
        let tool = Tool {
            name: "order.refund".into(),
            description: None,
            parameters: None,
            side_effects: vec![SideEffect::Write, SideEffect::Money],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        assert!(findings
            .iter()
            .all(|f| f.category != Category::MissingAuthz));
    }

    #[test]
    fn r3_fires_when_name_implies_side_effect_but_none_declared() {
        // `email.send` -- name has "send", a strong mutation verb. side_effects is
        // empty so the tool is lying about what it does.
        let tool = Tool {
            name: "email.send".into(),
            description: Some("Send an email.".into()),
            parameters: None,
            side_effects: vec![],      // <-- the offence: nothing declared
            auth_required: Some(true), // keep R2 quiet
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r3: Vec<_> = findings
            .iter()
            .filter(|f| f.category == Category::ExcessiveAgency)
            .filter(|f| f.id.contains("r3"))
            .collect();
        assert_eq!(r3.len(), 1, "expected R3 to fire, got {findings:?}");
        let f = r3[0];
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.tool.as_deref(), Some("email.send"));
        assert_eq!(f.mappings.owasp_llm, vec!["LLM08"]);
    }

    #[test]
    fn r3_silent_when_declared_side_effects_match_name() {
        let tool = Tool {
            name: "email.send".into(),
            description: Some("Send an email.".into()),
            parameters: None,
            side_effects: vec![SideEffect::Write, SideEffect::Network],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r3 = findings.iter().filter(|f| f.id.contains("r3")).count();
        assert_eq!(r3, 0);
    }

    #[test]
    fn r3_silent_for_read_only_name() {
        let tool = Tool {
            name: "order.get".into(),
            description: Some("Fetch one order by id.".into()),
            parameters: None,
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r3 = findings.iter().filter(|f| f.id.contains("r3")).count();
        assert_eq!(r3, 0);
    }

    #[test]
    fn r4_fires_on_unbounded_money_param() {
        let tool = Tool {
            name: "order.refund".into(),
            description: Some("Refund an order".into()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "order_id": { "type": "string", "maxLength": 64 },
                    "amount":   { "type": "number" }       // <-- offence: no maximum
                }
            })),
            side_effects: vec![SideEffect::Write, SideEffect::Money],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r4: Vec<_> = findings.iter().filter(|f| f.id.contains("r4")).collect();
        assert_eq!(r4.len(), 1, "expected one R4 finding; got {findings:?}");
        let f = r4[0];
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.category, Category::ExcessiveAgency);
        assert!(
            f.description.as_deref().unwrap_or("").contains("amount"),
            "description should name the offending param"
        );
    }

    #[test]
    fn r4_silent_when_money_param_has_maximum() {
        let tool = Tool {
            name: "order.refund".into(),
            description: None,
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "amount": { "type": "number", "maximum": 500 }
                }
            })),
            side_effects: vec![SideEffect::Write, SideEffect::Money],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r4 = findings.iter().filter(|f| f.id.contains("r4")).count();
        assert_eq!(r4, 0);
    }

    #[test]
    fn r4_silent_when_numeric_param_not_money_named() {
        let tool = Tool {
            name: "ping".into(),
            description: None,
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "timeout_ms": { "type": "number" }
                }
            })),
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r4 = findings.iter().filter(|f| f.id.contains("r4")).count();
        assert_eq!(r4, 0);
    }

    #[test]
    fn r5_fires_when_description_mentions_money_but_no_money_side_effect() {
        let tool = Tool {
            name: "billing.process".into(),
            description: Some("Process a customer payment for an invoice.".into()),
            parameters: None,
            side_effects: vec![SideEffect::Write], // <-- missing Money
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r5: Vec<_> = findings.iter().filter(|f| f.id.contains("r5")).collect();
        assert_eq!(r5.len(), 1, "expected one R5 finding; got {findings:?}");
        let f = r5[0];
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.category, Category::ExcessiveAgency);
        assert_eq!(f.mappings.owasp_llm, vec!["LLM08"]);
    }

    #[test]
    fn r5_silent_when_money_side_effect_is_declared() {
        let tool = Tool {
            name: "billing.process".into(),
            description: Some("Charge a customer for an invoice.".into()),
            parameters: None,
            side_effects: vec![SideEffect::Write, SideEffect::Money],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r5 = findings.iter().filter(|f| f.id.contains("r5")).count();
        assert_eq!(r5, 0);
    }

    #[test]
    fn r5_silent_when_description_has_no_money_words() {
        let tool = Tool {
            name: "users.list".into(),
            description: Some("List all users.".into()),
            parameters: None,
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r5 = findings.iter().filter(|f| f.id.contains("r5")).count();
        assert_eq!(r5, 0);
    }

    #[test]
    fn r5_silent_on_resource_subscription_not_money() {
        // Real false positive found scanning the MCP `everything` server:
        // "resource subscription updates" is not a paid subscription.
        let tool = Tool {
            name: "toggle-subscriber-updates".into(),
            description: Some("Toggles simulated resource subscription updates on or off.".into()),
            parameters: None,
            side_effects: vec![],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        assert_eq!(
            findings.iter().filter(|f| f.id.contains("r5")).count(),
            0,
            "a resource subscription is not a money operation"
        );
    }

    #[test]
    fn r6_silent_on_downloadable_output_not_external_fetch() {
        // Real false positive found scanning the MCP `filesystem`/everything
        // gzip tool: returning downloadable *output* is not fetching external
        // attacker-controlled content.
        let tool = Tool {
            name: "gzip-file".into(),
            description: Some(
                "Compresses a local file and returns the data as a resource, \
                 allowing it to be downloaded."
                    .into(),
            ),
            parameters: None,
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        assert_eq!(
            findings.iter().filter(|f| f.id.contains("r6")).count(),
            0,
            "downloadable output is not an external-fetch / indirect-injection surface"
        );
    }

    #[test]
    fn r6_fires_when_description_implies_external_fetch() {
        let tool = Tool {
            name: "summarize".into(),
            description: Some("Fetch the given URL and return a short summary of the page.".into()),
            parameters: Some(json!({
                "type": "object",
                "properties": { "url": { "type": "string", "maxLength": 500 } }
            })),
            side_effects: vec![SideEffect::Network, SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r6: Vec<_> = findings.iter().filter(|f| f.id.contains("r6")).collect();
        assert_eq!(r6.len(), 1, "expected one R6 finding; got {findings:?}");
        let f = r6[0];
        assert_eq!(f.category, Category::IndirectInjection);
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.mappings.owasp_llm, vec!["LLM01"]);
    }

    #[test]
    fn r6_silent_for_local_only_description() {
        let tool = Tool {
            name: "math.add".into(),
            description: Some("Add two integers and return the sum.".into()),
            parameters: None,
            side_effects: vec![SideEffect::Read],
            auth_required: Some(true),
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        let r6 = findings.iter().filter(|f| f.id.contains("r6")).count();
        assert_eq!(r6, 0);
    }

    #[test]
    fn r2_silent_for_read_only_tool() {
        let tool = Tool {
            name: "order.read".into(),
            description: Some("List recent orders.".into()),
            parameters: None,
            side_effects: vec![SideEffect::Read],
            auth_required: None,
            rate_limited: None,
        };
        let findings = classify(&inventory_with_one_tool(tool));
        assert!(findings
            .iter()
            .all(|f| f.category != Category::MissingAuthz));
    }

    #[test]
    fn r1_silent_when_no_string_params() {
        let tool = Tool {
            name: "math.sum".into(),
            description: None,
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "a": {"type": "number"},
                    "b": {"type": "number"}
                }
            })),
            side_effects: vec![],
            auth_required: None,
            rate_limited: None,
        };
        assert_eq!(classify(&inventory_with_one_tool(tool)).len(), 0);
    }
}
