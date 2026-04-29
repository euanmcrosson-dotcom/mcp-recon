//! Schema-aware adversarial fuzzer.
//!
//! v0.1 strategy (per docs/SPEC.md §"Fuzzing strategy"):
//!
//! - Generates inputs from the JSON Schema across six axes:
//!   boundary values, type confusion, encoding tricks, path traversal,
//!   URL hostility, and schema violation.
//! - Default budget: 200 calls per tool.
//! - Deterministic: seeded PRNG (default seed `0xC0FFEE`).
//!
//! The fuzzer **records what happened**; it does not classify whether a
//! particular fuzz "succeeded" or "failed." Outcome detection is human
//! review or the v0.2 ML classifier.

use serde::{Deserialize, Serialize};

/// Default seed for the deterministic fuzz PRNG.
pub const DEFAULT_SEED: u64 = 0xC0FFEE;

/// Default per-tool fuzz budget (number of calls).
pub const DEFAULT_BUDGET: usize = 200;

/// One recorded fuzz interaction: input + observed response shape.
/// The wire shape from the underlying tool is opaque to the fuzzer; the
/// downstream classifier and reporter interpret the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzCall {
    /// Tool name being fuzzed.
    pub tool: String,
    /// Which axis of the fuzz strategy generated this input.
    pub axis: FuzzAxis,
    /// The arguments that were sent (JSON value).
    pub arguments: serde_json::Value,
    /// The response shape — `ok` (the underlying tool accepted),
    /// `protocol_error` (MCP returned an error), or `runtime_error`
    /// (transport / process error).
    pub outcome: FuzzOutcome,
}

/// Categorical fuzz axis. v0.1 has six.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FuzzAxis {
    /// Empty / max-length / numeric edge values.
    BoundaryValues,
    /// String when number expected, etc.
    TypeConfusion,
    /// Percent-encoding, Unicode homographs, null bytes.
    EncodingTricks,
    /// `..`, `%2e%2e`, mixed separators (for path-shaped args).
    PathTraversal,
    /// Userinfo splitting, IDN homograph, scheme tricks (for URL args).
    UrlHostility,
    /// Extra fields, missing required fields, wrong types in nested.
    SchemaViolation,
}

/// What the underlying tool / transport did with the fuzz input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FuzzOutcome {
    /// Tool returned a normal-shaped success response.
    Ok {
        /// Truncated string of the response for human review.
        snippet: String,
    },
    /// MCP returned an error (per-tool / per-call). Expected for most
    /// adversarial inputs; this is the *bounded* result.
    ProtocolError {
        /// Error message string from the MCP layer.
        message: String,
    },
    /// Transport or process-level error (the worst kind — a fuzzer that
    /// crashes the server hits this).
    RuntimeError {
        /// Error message string.
        message: String,
    },
}

// v0.1 week 2 will add the `fuzz()` entry point and the per-axis input
// generators. The types above are the wire shape; the implementation
// follows.
