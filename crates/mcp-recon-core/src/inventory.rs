//! McpInventory — the v1 input format consumed by the classifier.
//!
//! An inventory is the result of (optionally) listing tools from one or
//! more MCP servers and recording their declared metadata. It's
//! deliberately a *snapshot*: the classifier is deterministic over the
//! snapshot and produces the same findings every run.
//!
//! Wire format:
//!
//! ```json
//! {
//!   "schema": "mcp-recon.inventory.v1",
//!   "servers": [
//!     {
//!       "name": "shopify-mcp",
//!       "transport": "stdio",
//!       "tools": [
//!         {
//!           "name": "order.refund",
//!           "description": "Issue a refund on an order.",
//!           "parameters": { ...JSON-Schema... },
//!           "side_effects": ["write", "money"],
//!           "auth_required": true,
//!           "rate_limited": false
//!         }
//!       ]
//!     }
//!   ]
//! }
//! ```

use serde::{Deserialize, Serialize};

/// Schema-version tag for the inventory wire format.
pub const INVENTORY_SCHEMA: &str = "mcp-recon.inventory.v1";

/// Top-level inventory document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpInventory {
    /// Schema-version tag; should equal [`INVENTORY_SCHEMA`].
    pub schema: String,
    /// Servers contributing tools to this inventory.
    pub servers: Vec<McpServer>,
}

/// Per-server entry in an inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    /// Logical name of the server (e.g. "shopify-mcp").
    pub name: String,
    /// Transport used to talk to the server. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    /// Tools the server advertised.
    #[serde(default)]
    pub tools: Vec<Tool>,
}

/// Transport the server runs on. Matches capframe.findings.v1's Transport enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// stdio (typical for local MCP servers).
    Stdio,
    /// HTTP (request/response).
    Http,
    /// Server-sent events.
    Sse,
    /// WebSocket.
    Websocket,
}

/// One tool entry in an inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name (e.g. "order.refund").
    pub name: String,
    /// Free-form description supplied by the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Parameter schema (JSON Schema, kept as opaque JSON for flexibility).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    /// Declared side effects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub side_effects: Vec<SideEffect>,
    /// Whether the tool requires authentication (server-declared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_required: Option<bool>,
    /// Whether the tool is rate-limited (server-declared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limited: Option<bool>,
}

/// Side effects a tool can have. Mirrors capframe.findings.v1's SideEffect enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    /// Reads state.
    Read,
    /// Writes state.
    Write,
    /// Issues network requests.
    Network,
    /// Touches the filesystem.
    Filesystem,
    /// Executes subprocesses.
    Execute,
    /// Moves money.
    Money,
    /// Irreversible effect (deletions, transfers).
    Irreversible,
}
