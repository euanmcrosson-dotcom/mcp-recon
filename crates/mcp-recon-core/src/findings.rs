//! Findings — the classifier output, shaped to match capframe.findings.v1.
//!
//! These types are deliberately a separate definition from capframe-findings;
//! they're owned by mcp-recon-core so the crate has no upstream Capframe
//! dependency. The wire JSON shape is identical.

use serde::{Deserialize, Serialize};

/// One detected issue with a tool surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    /// Stable identifier, suitable for diffing across scans.
    pub id: String,
    /// Severity assigned by the rule that produced this finding.
    pub severity: Severity,
    /// Class of issue.
    pub category: Category,
    /// Short human-readable title (<= 200 chars).
    pub title: String,
    /// Longer description / evidence summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Name of the tool this finding relates to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Remediation hint shown in reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    /// Compliance-framework mappings.
    #[serde(default, skip_serializing_if = "Mappings::is_empty")]
    pub mappings: Mappings,
    /// CAST (Capframe Agent Security Taxonomy) categories, derived from
    /// `category`. Populated centrally by the classifier via [`category_to_cast`]
    /// so every emitted finding carries its CAST tag — not just the ones that
    /// pass through the `capframe find` CLI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cast_category: Vec<CastCategory>,
}

/// Severity ordering (Info < Low < Medium < High < Critical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational; not actionable on its own.
    Info,
    /// Low risk.
    Low,
    /// Medium risk.
    Medium,
    /// High risk; should be remediated.
    High,
    /// Critical risk; remediate immediately.
    Critical,
}

/// Class of finding. Stable across scanner implementations — mirrors the
/// `Category` enum in capframe.findings.v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Indirect prompt injection surface (LLM01).
    IndirectInjection,
    /// Excessive agency — tool can do more than the user authorized (LLM08).
    ExcessiveAgency,
    /// Input is not constrained by length / range / enum.
    UnconstrainedInput,
    /// Side-effect tool with no auth requirement.
    MissingAuthz,
    /// Output handling is unsafe (HTML-injected outputs, etc.).
    InsecureOutputHandling,
    /// Tool surface leaks secrets.
    SecretExposure,
    /// Multiple tools share an ambiguous name.
    ToolNamingConflict,
    /// Deserialization-attack surface.
    Deserialization,
    /// Server-side request forgery surface.
    SsrfSurface,
    /// Filesystem egress.
    FilesystemEgress,
    /// Network egress.
    NetworkEgress,
    /// Tool ships with an untrusted dep.
    UntrustedDependency,
    /// Anything else.
    Other,
}

/// CAST v0.1 risk category (Capframe Agent Security Taxonomy). Wire representation
/// (`"CAST-01"` …) is identical to `capframe_findings::CastCategory` so findings
/// round-trip into the Capframe report and leaderboard unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CastCategory {
    /// CAST-01 — Tool Capability Excess.
    #[serde(rename = "CAST-01")]
    Cast01,
    /// CAST-02 — Indirect Injection via Tool Output.
    #[serde(rename = "CAST-02")]
    Cast02,
    /// CAST-03 — Insufficient Capability Scoping.
    #[serde(rename = "CAST-03")]
    Cast03,
    /// CAST-04 — Tool Metadata Poisoning.
    #[serde(rename = "CAST-04")]
    Cast04,
    /// CAST-05 — Capability Boundary Violation.
    #[serde(rename = "CAST-05")]
    Cast05,
    /// CAST-06 — Cross-Tool Propagation.
    #[serde(rename = "CAST-06")]
    Cast06,
    /// CAST-07 — Persistent State Poisoning.
    #[serde(rename = "CAST-07")]
    Cast07,
    /// CAST-08 — Uncontrolled Tool Invocation.
    #[serde(rename = "CAST-08")]
    Cast08,
    /// CAST-09 — Multi-Agent Authority Delegation.
    #[serde(rename = "CAST-09")]
    Cast09,
}

/// Mapping from a finding [`Category`] to its CAST categories — the source of
/// truth for CAST tagging in the producer/classifier path. Wire-identical to the
/// parallel mapping in the `capframe find` CLI, so both paths tag findings the same.
pub fn category_to_cast(c: Category) -> Vec<CastCategory> {
    use CastCategory::*;
    match c {
        Category::ExcessiveAgency => vec![Cast01],
        Category::IndirectInjection => vec![Cast02],
        Category::UnconstrainedInput => vec![Cast03],
        Category::MissingAuthz => vec![Cast03],
        Category::InsecureOutputHandling => vec![Cast01],
        Category::SecretExposure => vec![Cast01],
        Category::SsrfSurface => vec![Cast02],
        Category::FilesystemEgress => vec![Cast01],
        Category::NetworkEgress => vec![Cast02],
        Category::ToolNamingConflict => vec![Cast04],
        Category::UntrustedDependency => vec![Cast04],
        Category::Deserialization => vec![Cast01],
        Category::Other => vec![],
    }
}

/// Compliance-framework mappings attached to a finding.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mappings {
    /// OWASP LLM Top 10 IDs (e.g. `LLM01`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owasp_llm: Vec<String>,
    /// NIST AI RMF IDs (e.g. `MEASURE-2.3`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nist_rmf: Vec<String>,
    /// MITRE ATLAS IDs (e.g. `T0051`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mitre_atlas: Vec<String>,
}

impl Mappings {
    /// True if no mapping IDs are recorded.
    pub fn is_empty(&self) -> bool {
        self.owasp_llm.is_empty() && self.nist_rmf.is_empty() && self.mitre_atlas.is_empty()
    }
}
