//! Authority classifier.
//!
//! v0.1 strategy (per docs/SPEC.md §"Classification rules"): rule-based,
//! not ML-based. Two signals:
//!
//! 1. Tool description matched against curated regex keywords per
//!    data-class.
//! 2. inputSchema inspected for argument types that suggest ambient
//!    authority (path-shaped strings, URL-shaped strings, command-shaped
//!    arrays).
//!
//! The crossing of (a) user-controllable string args AND (b) a side-
//! effecting verb in the description is the **confused-deputy
//! candidate** flag — the load-bearing signal.

use serde::{Deserialize, Serialize};

/// Top-level data-class assignment. v0.1 has seven categories chosen to
/// cover the surfaces capnagent already has examples for.
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

/// Heuristic authority level — what the tool can do once invoked.
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

/// One classification entry per tool. `confused_deputy_candidate` is the
/// load-bearing flag: a high-confidence signal that the tool takes
/// user-controllable input AND has a side effect.
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
    /// Free-form rationale string for human review (which rules fired,
    /// which keywords matched). Not machine-actionable.
    pub rationale: String,
}

// v0.1 week 3 will populate this module with the rule-table-driven
// classify() function. For the scaffold, the public types above are
// the wire shape; nothing else is needed yet.
