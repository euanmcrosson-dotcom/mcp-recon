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
