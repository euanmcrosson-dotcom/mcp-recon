//! mcp-recon-core — rule-based authority classifier + schema-aware fuzzer
//! for MCP tool surfaces.
//!
//! This crate is the deterministic, language-agnostic core. The CLI in
//! `packages/mcp-recon-cli` calls into it via WASM bindings (added in
//! v0.1 week 3). The TS layer owns the MCP protocol; this crate owns
//! everything that benefits from being in Rust — the classifier rule
//! engine, the schema-aware fuzz generator, and the report renderer.
//!
//! v0.1 status: scaffold. The public types below match the wire shape
//! the CLI emits today (`mcp-recon enumerate`); the classifier and
//! fuzzer modules are stubs documented per docs/SPEC.md.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod classifier;
pub mod fuzzer;
pub mod report;

pub use classifier::{Classification, DataClass, AuthorityLevel};

/// Schema-version tag emitted by `mcp-recon classify` (matches the
/// CLI's `INVENTORY_SCHEMA` for inventory documents).
pub const CLASSIFICATION_SCHEMA: &str = "mcp-recon/v0.1/classification";

/// Schema-version tag emitted by `mcp-recon fuzz`.
pub const FUZZ_SCHEMA: &str = "mcp-recon/v0.1/fuzz";

/// Schema-version tag emitted by `mcp-recon report` (the JSON
/// front-matter; the Markdown body is rendered separately).
pub const REPORT_SCHEMA: &str = "mcp-recon/v0.1/report";
