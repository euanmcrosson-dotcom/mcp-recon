//! Adapters from third-party tool-use formats to [`McpInventory`].
//!
//! mcp-recon's classifier operates on `mcp-recon.inventory.v1`, but most
//! agent platforms in the wild don't speak MCP — they describe tools in
//! their own provider-specific format. The adapters here translate those
//! formats into an `McpInventory` so the existing deterministic rules run
//! unchanged.
//!
//! Supported today:
//! - **Anthropic tool-use** (`[{ name, description, input_schema }, …]`)
//! - **OpenAI function-calling**, both the current chat-completions
//!   `[{ type: "function", function: { name, description, parameters } }, …]`
//!   form and the deprecated bare-function `[{ name, description, parameters }, …]`
//! - **LangChain `BaseTool`** dumps (`[{ name, description, args_schema }, …]`),
//!   with a fallback for the older bare-`args` field
//!
//! Each adapter accepts either a bare array of tool entries or a `{ tools: [...] }`
//! wrapper object, since both shapes appear in real-world configs and request
//! bodies.
//!
//! ## What is lost in translation
//!
//! Neither Anthropic's nor OpenAI's tool format declares `side_effects` or
//! `auth_required`. The adapter leaves both fields empty / `None` and relies on
//! the classifier to infer authority signals from the tool name + description
//! via R3 (name implies mutation), R5 (description mentions money), R6
//! (description implies external fetch), and R7 (code execution). That is the
//! correct fail-open posture: an undeclared field is not a denial of risk.

use serde_json::Value;

use crate::inventory::{McpInventory, McpServer, Tool, Transport, INVENTORY_SCHEMA};

/// Failure modes the adapters can surface.
#[derive(Debug)]
pub enum AdapterError {
    /// The input JSON was neither an array of tools nor a `{ tools: [...] }`
    /// wrapper. Carries the type tag we observed.
    UnexpectedShape(&'static str),
    /// A tool entry was not a JSON object.
    ToolNotAnObject {
        /// Zero-based index of the offending entry.
        index: usize,
    },
    /// A tool entry was missing the required `name` field, or it was not a
    /// string. Tools without names can't be classified or named in caveats.
    MissingName {
        /// Zero-based index of the offending entry.
        index: usize,
    },
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedShape(ty) => write!(
                f,
                "expected an array of tools or a {{ \"tools\": [...] }} wrapper, got JSON {ty}"
            ),
            Self::ToolNotAnObject { index } => {
                write!(f, "tool at index {index} is not a JSON object")
            }
            Self::MissingName { index } => write!(
                f,
                "tool at index {index} has no `name` field (or it is not a string)"
            ),
        }
    }
}

impl std::error::Error for AdapterError {}

/// Translate an Anthropic tool-use payload into an `McpInventory`.
///
/// Accepts either `[{ name, description, input_schema }, ...]` or
/// `{ tools: [...] }`. The `server_name` becomes the only [`McpServer`]'s
/// `name`; the transport is recorded as [`Transport::Stdio`] since Anthropic
/// tools are local to the agent-side process.
pub fn from_anthropic_tools(payload: &Value, server_name: &str) -> Result<McpInventory, AdapterError> {
    let arr = unwrap_tools_array(payload)?;
    let mut tools = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        let obj = entry.as_object().ok_or(AdapterError::ToolNotAnObject { index: i })?;
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(AdapterError::MissingName { index: i })?
            .to_string();
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        // Anthropic uses `input_schema`. Always carry the entire JSON Schema
        // through unchanged so R1 / R4 can inspect properties + constraints.
        let parameters = obj.get("input_schema").cloned();
        tools.push(Tool {
            name,
            description,
            parameters,
            side_effects: Vec::new(),
            auth_required: None,
            rate_limited: None,
        });
    }
    Ok(wrap_in_inventory(server_name, tools))
}

/// Translate an OpenAI function-calling / tools payload into an
/// `McpInventory`.
///
/// Accepts both the current `[{ type: "function", function: {...} }, ...]`
/// chat-completions shape and the deprecated bare-function
/// `[{ name, description, parameters }, ...]` form. A mixed input is fine —
/// the adapter unwraps per entry.
pub fn from_openai_tools(payload: &Value, server_name: &str) -> Result<McpInventory, AdapterError> {
    let arr = unwrap_tools_array(payload)?;
    let mut tools = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        let obj = entry.as_object().ok_or(AdapterError::ToolNotAnObject { index: i })?;

        // Current shape wraps the function under `function`; legacy shape is
        // flat. Detect by presence of the wrapper.
        let fn_obj = if obj.get("type").and_then(|v| v.as_str()) == Some("function") {
            obj.get("function")
                .and_then(|v| v.as_object())
                .ok_or(AdapterError::ToolNotAnObject { index: i })?
        } else {
            obj
        };

        let name = fn_obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(AdapterError::MissingName { index: i })?
            .to_string();
        let description = fn_obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let parameters = fn_obj.get("parameters").cloned();

        tools.push(Tool {
            name,
            description,
            parameters,
            side_effects: Vec::new(),
            auth_required: None,
            rate_limited: None,
        });
    }
    Ok(wrap_in_inventory(server_name, tools))
}

/// Translate a LangChain `BaseTool` JSON dump into an `McpInventory`.
///
/// Accepts the conventional dump `[{ name, description, args_schema }, ...]`
/// (what `tool.model_dump()` plus `args_schema=tool.args` produces). For
/// looser serializations that flatten the schema into `args` instead of
/// `args_schema`, the adapter falls back to that field. Like the other
/// providers, both bare-array and `{ tools: [...] }` wrappers are accepted.
///
/// If the input is from a stack that serializes LangChain tools via
/// `convert_to_openai_tool()`, use `from_openai_tools` instead — that output
/// matches OpenAI's wire format byte-for-byte.
pub fn from_langchain_tools(
    payload: &Value,
    server_name: &str,
) -> Result<McpInventory, AdapterError> {
    let arr = unwrap_tools_array(payload)?;
    let mut tools = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        let obj = entry.as_object().ok_or(AdapterError::ToolNotAnObject { index: i })?;
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(AdapterError::MissingName { index: i })?
            .to_string();
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        // Canonical LangChain field is `args_schema`; some dumps use the
        // shorter `args` (the property dict alone). Prefer the schema.
        let parameters = obj
            .get("args_schema")
            .or_else(|| obj.get("args"))
            .cloned();
        tools.push(Tool {
            name,
            description,
            parameters,
            side_effects: Vec::new(),
            auth_required: None,
            rate_limited: None,
        });
    }
    Ok(wrap_in_inventory(server_name, tools))
}

/// Accept either a bare JSON array or a `{ tools: [...] }` wrapper. Returns
/// the array; everything else is an `UnexpectedShape` error.
fn unwrap_tools_array(payload: &Value) -> Result<&Vec<Value>, AdapterError> {
    if let Some(arr) = payload.as_array() {
        return Ok(arr);
    }
    if let Some(obj) = payload.as_object() {
        if let Some(arr) = obj.get("tools").and_then(|v| v.as_array()) {
            return Ok(arr);
        }
        return Err(AdapterError::UnexpectedShape("object without a `tools` array"));
    }
    Err(AdapterError::UnexpectedShape(value_type_name(payload)))
}

fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn wrap_in_inventory(server_name: &str, tools: Vec<Tool>) -> McpInventory {
    McpInventory {
        schema: INVENTORY_SCHEMA.into(),
        servers: vec![McpServer {
            name: server_name.to_string(),
            transport: Some(Transport::Stdio),
            tools,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify;
    use serde_json::json;

    // ── Anthropic ──────────────────────────────────────────────────────────

    #[test]
    fn anthropic_array_of_tools_maps_to_inventory() {
        let payload = json!([
            {
                "name": "get_weather",
                "description": "Get the current weather in a given location",
                "input_schema": {
                    "type": "object",
                    "properties": { "location": { "type": "string" } },
                    "required": ["location"]
                }
            }
        ]);
        let inv = from_anthropic_tools(&payload, "anthropic-bot").expect("adapter");
        assert_eq!(inv.schema, INVENTORY_SCHEMA);
        assert_eq!(inv.servers.len(), 1);
        let server = &inv.servers[0];
        assert_eq!(server.name, "anthropic-bot");
        assert_eq!(server.transport, Some(Transport::Stdio));
        assert_eq!(server.tools.len(), 1);
        let t = &server.tools[0];
        assert_eq!(t.name, "get_weather");
        assert_eq!(t.description.as_deref(), Some("Get the current weather in a given location"));
        // input_schema is carried through unchanged so the classifier can read it.
        assert_eq!(t.parameters.as_ref().unwrap()["properties"]["location"]["type"], "string");
        // No declared side-effects / auth — left for the rules to infer.
        assert!(t.side_effects.is_empty());
        assert_eq!(t.auth_required, None);
    }

    #[test]
    fn anthropic_object_wrapped_tools_field_works() {
        let payload = json!({ "tools": [
            { "name": "list_files", "input_schema": { "type": "object", "properties": {} } }
        ]});
        let inv = from_anthropic_tools(&payload, "wrap").expect("adapter");
        assert_eq!(inv.servers[0].tools[0].name, "list_files");
    }

    #[test]
    fn anthropic_tool_without_input_schema_yields_none_parameters() {
        let payload = json!([{ "name": "ping" }]);
        let inv = from_anthropic_tools(&payload, "s").expect("adapter");
        assert!(inv.servers[0].tools[0].parameters.is_none());
    }

    #[test]
    fn anthropic_missing_name_is_a_clean_error() {
        let payload = json!([{ "description": "nameless" }]);
        let err = from_anthropic_tools(&payload, "s").expect_err("must fail");
        assert!(matches!(err, AdapterError::MissingName { index: 0 }));
    }

    #[test]
    fn anthropic_non_object_tool_entry_is_a_clean_error() {
        let payload = json!(["not a tool"]);
        let err = from_anthropic_tools(&payload, "s").expect_err("must fail");
        assert!(matches!(err, AdapterError::ToolNotAnObject { index: 0 }));
    }

    #[test]
    fn anthropic_top_level_garbage_is_a_clean_error() {
        for garbage in [json!(null), json!(42), json!("nope"), json!({})] {
            assert!(from_anthropic_tools(&garbage, "s").is_err(), "{garbage} should fail");
        }
    }

    // ── OpenAI (current and legacy shapes) ─────────────────────────────────

    #[test]
    fn openai_current_function_wrapper_shape_maps_to_inventory() {
        let payload = json!([
            {
                "type": "function",
                "function": {
                    "name": "search",
                    "description": "Search the web",
                    "parameters": {
                        "type": "object",
                        "properties": { "query": { "type": "string" } }
                    }
                }
            }
        ]);
        let inv = from_openai_tools(&payload, "openai-bot").expect("adapter");
        let t = &inv.servers[0].tools[0];
        assert_eq!(t.name, "search");
        assert_eq!(t.description.as_deref(), Some("Search the web"));
        assert!(t.parameters.is_some());
    }

    #[test]
    fn openai_legacy_bare_function_shape_still_works() {
        let payload = json!([
            { "name": "legacy", "description": "old shape", "parameters": { "type": "object" } }
        ]);
        let inv = from_openai_tools(&payload, "s").expect("adapter");
        assert_eq!(inv.servers[0].tools[0].name, "legacy");
        assert!(inv.servers[0].tools[0].parameters.is_some());
    }

    #[test]
    fn openai_mixed_current_and_legacy_in_same_array() {
        let payload = json!([
            { "type": "function", "function": { "name": "a" } },
            { "name": "b" }
        ]);
        let inv = from_openai_tools(&payload, "s").expect("adapter");
        let names: Vec<&str> = inv.servers[0]
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn openai_function_wrapper_with_missing_function_body_errors() {
        let payload = json!([{ "type": "function" }]);
        let err = from_openai_tools(&payload, "s").expect_err("must fail");
        assert!(matches!(err, AdapterError::ToolNotAnObject { index: 0 }));
    }

    // ── LangChain ──────────────────────────────────────────────────────────

    #[test]
    fn langchain_args_schema_field_is_the_canonical_path() {
        let payload = json!([
            {
                "name": "calculator",
                "description": "Evaluate a math expression.",
                "args_schema": {
                    "type": "object",
                    "properties": { "expr": { "type": "string", "maxLength": 256 } },
                    "required": ["expr"]
                }
            }
        ]);
        let inv = from_langchain_tools(&payload, "lc").expect("adapter");
        let t = &inv.servers[0].tools[0];
        assert_eq!(t.name, "calculator");
        assert_eq!(t.description.as_deref(), Some("Evaluate a math expression."));
        assert_eq!(t.parameters.as_ref().unwrap()["properties"]["expr"]["maxLength"], 256);
    }

    #[test]
    fn langchain_falls_back_to_args_when_no_args_schema() {
        // Some looser dumps put the property block under `args` instead of
        // wrapping it as a full `args_schema`.
        let payload = json!([
            {
                "name": "calculator",
                "args": { "expr": { "type": "string" } }
            }
        ]);
        let inv = from_langchain_tools(&payload, "lc").expect("adapter");
        let t = &inv.servers[0].tools[0];
        assert_eq!(
            t.parameters.as_ref().unwrap()["expr"]["type"],
            "string"
        );
    }

    #[test]
    fn langchain_object_wrapped_tools_field_works() {
        let payload = json!({ "tools": [
            { "name": "search", "args_schema": { "type": "object", "properties": {} } }
        ]});
        let inv = from_langchain_tools(&payload, "lc").expect("adapter");
        assert_eq!(inv.servers[0].tools[0].name, "search");
    }

    #[test]
    fn langchain_tool_without_schema_yields_none_parameters() {
        let payload = json!([{ "name": "ping" }]);
        let inv = from_langchain_tools(&payload, "lc").expect("adapter");
        assert!(inv.servers[0].tools[0].parameters.is_none());
    }

    #[test]
    fn langchain_missing_name_is_a_clean_error() {
        let payload = json!([{ "description": "nameless" }]);
        let err = from_langchain_tools(&payload, "lc").expect_err("must fail");
        assert!(matches!(err, AdapterError::MissingName { index: 0 }));
    }

    #[test]
    fn langchain_inventory_runs_through_classifier_and_fires_rules() {
        // LangChain agents in the wild often expose a shell tool — should
        // light up R7. Also includes an unbounded refund amount → R4.
        let payload = json!([
            {
                "name": "shell",
                "description": "Run shell commands on the host system.",
                "args_schema": {
                    "type": "object",
                    "properties": { "command": { "type": "string", "maxLength": 4096 } }
                }
            },
            {
                "name": "refund_payment",
                "description": "Refund a payment by ID.",
                "args_schema": {
                    "type": "object",
                    "properties": {
                        "payment_id": { "type": "string", "maxLength": 64 },
                        "amount":     { "type": "number" }
                    }
                }
            }
        ]);
        let inv = from_langchain_tools(&payload, "lc").expect("adapter");
        let findings = classify(&inv);
        let ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
        assert!(
            ids.iter().any(|id| id.contains("r7") && id.contains("shell")),
            "R7 should fire on the shell tool; got {ids:?}"
        );
        assert!(
            ids.iter().any(|id| id.contains("r4") && id.contains("refund_payment")),
            "R4 should fire on unbounded refund amount; got {ids:?}"
        );
    }

    // ── End-to-end: adapter output runs through the classifier ─────────────

    #[test]
    fn anthropic_inventory_runs_through_classifier_and_fires_rules() {
        // R7 should fire on a tool whose description says "execute …".
        let payload = json!([
            {
                "name": "run_python",
                "description": "Execute Python code in a sandbox and return stdout.",
                "input_schema": {
                    "type": "object",
                    "properties": { "code": { "type": "string" } }
                }
            }
        ]);
        let inv = from_anthropic_tools(&payload, "s").expect("adapter");
        let findings = classify(&inv);
        assert!(
            findings.iter().any(|f| f.id.contains("r7")),
            "R7 should fire on the python-execution tool; got {findings:#?}"
        );
        // R1 should also fire — `code` has no maxLength.
        assert!(
            findings.iter().any(|f| f.id.contains("r1")),
            "R1 should fire on unconstrained `code` string; got {findings:#?}"
        );
    }

    #[test]
    fn openai_inventory_runs_through_classifier_and_fires_rules() {
        // A refund tool with an unbounded amount: R4 should fire.
        let payload = json!([
            {
                "type": "function",
                "function": {
                    "name": "order.refund",
                    "description": "Refund an order",
                    "parameters": {
                        "type": "object",
                        "properties": { "amount": { "type": "number" } }
                    }
                }
            }
        ]);
        let inv = from_openai_tools(&payload, "s").expect("adapter");
        let findings = classify(&inv);
        assert!(
            findings.iter().any(|f| f.id.contains("r4")),
            "R4 should fire on unbounded refund amount; got {findings:#?}"
        );
    }
}
