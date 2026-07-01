//! mcp-recon-core — the deterministic, rule-based core of mcp-recon.
//!
//! Shipping today: the [`classifier`] (rules R1-R7 over an [`inventory`]
//! snapshot, emitting [`findings`]) and [`caveats`] (the Find -> Bind handoff:
//! findings to capnagent-ready issuance plans). The CLI (`mcp-recon-cli`) owns
//! live enumeration (stdio + HTTP) and calls into this crate directly.
//!
//! The [`fuzzer`] and [`report`] modules are documented stubs for planned work
//! (see docs/SPEC.md); they are not wired into the shipped CLI.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod adapters;
pub mod caveats;
pub mod classifier;
pub mod findings;
pub mod fuzzer;
pub mod inventory;
pub mod report;

pub use adapters::{from_anthropic_tools, from_langchain_tools, from_openai_tools, AdapterError};
pub use caveats::{caveats_v1, CaveatArtifact, CaveatPlan, CAVEATS_SCHEMA};
pub use classifier::{classify, unbounded_money_params, AuthorityLevel, Classification, DataClass};
pub use findings::{category_to_cast, CastCategory, Category, Finding, Mappings, Severity};
pub use inventory::{McpInventory, McpServer, SideEffect, Tool, Transport, INVENTORY_SCHEMA};

/// Schema-version tag emitted by `mcp-recon classify` (matches the
/// CLI's `INVENTORY_SCHEMA` for inventory documents).
pub const CLASSIFICATION_SCHEMA: &str = "mcp-recon/v0.1/classification";

/// Schema-version tag emitted by `mcp-recon fuzz`.
pub const FUZZ_SCHEMA: &str = "mcp-recon/v0.1/fuzz";

/// Schema-version tag emitted by `mcp-recon report` (the JSON
/// front-matter; the Markdown body is rendered separately).
pub const REPORT_SCHEMA: &str = "mcp-recon/v0.1/report";
