//! Build a `findings.v2`-shaped JSON document for one producer run.
//!
//! findings.v2 lives in the capframe repo (`schemas/findings.v2.json`)
//! and is not a Rust crate dep here (mcp-recon and capframe both ship
//! their own copy of the schema types to avoid a circular path dep).
//! We emit the v2 envelope directly via `serde_json::json!` so that
//! capframe-findings remains the canonical source of truth.

use anyhow::Result;
use mcp_recon_core::{Finding, McpInventory};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

pub const SCHEMA_VERSION_V2: &str = "capframe.findings.v2";

#[derive(Debug, Clone, Copy)]
pub enum ProducerSource {
    Registry,
    /// Wired in a follow-up phase when the HTTP producer lands.
    #[allow(dead_code)]
    Http,
    /// Wired in a follow-up phase when the sandbox producer lands.
    #[allow(dead_code)]
    Sandbox,
    /// Used by hand-authored inventories; not produced by `mcp-recon producer`.
    #[allow(dead_code)]
    File,
}

impl ProducerSource {
    fn as_str(self) -> &'static str {
        match self {
            ProducerSource::Registry => "registry",
            ProducerSource::Http => "http",
            ProducerSource::Sandbox => "sandbox",
            ProducerSource::File => "file",
        }
    }
}

pub struct EnvelopeInput<'a> {
    pub scan_id: Uuid,
    pub scanned_at: OffsetDateTime,
    pub scanner_name: &'a str,
    pub scanner_version: &'a str,
    pub handle: String,
    pub source: ProducerSource,
    pub repo_url: Option<String>,
    pub name: Option<String>,
    pub inventory: &'a McpInventory,
    pub findings: Vec<Finding>,
}

pub fn build(input: EnvelopeInput<'_>) -> Result<Value> {
    let scanned_at = input.scanned_at.format(&Rfc3339)?;
    let summary = summarize(&input.findings);

    let tools = input
        .inventory
        .servers
        .iter()
        .flat_map(|s| s.tools.iter())
        .collect::<Vec<_>>();
    let tools_json = serde_json::to_value(&tools)?;

    let findings_json = serde_json::to_value(&input.findings)?;

    let transport = input
        .inventory
        .servers
        .first()
        .and_then(|s| s.transport)
        .map(|t| serde_json::to_value(t).expect("Transport serde never fails"));

    let mut server = serde_json::Map::new();
    server.insert("handle".into(), json!(input.handle));
    server.insert("kind".into(), json!("mcp_server"));
    server.insert("source".into(), json!(input.source.as_str()));
    if let Some(repo) = input.repo_url {
        server.insert("repo_url".into(), json!(repo));
    }
    if let Some(name) = input.name {
        server.insert("name".into(), json!(name));
    }
    if let Some(t) = transport {
        server.insert("transport".into(), t);
    }

    Ok(json!({
        "schema_version": SCHEMA_VERSION_V2,
        "scan_id": input.scan_id.to_string(),
        "scanned_at": scanned_at,
        "scanner": {
            "name": input.scanner_name,
            "version": input.scanner_version,
        },
        "server": Value::Object(server),
        "tools": tools_json,
        "findings": findings_json,
        "summary": summary,
    }))
}

fn summarize(findings: &[Finding]) -> Value {
    let mut info = 0_u32;
    let mut low = 0_u32;
    let mut medium = 0_u32;
    let mut high = 0_u32;
    let mut critical = 0_u32;
    let mut by_category: std::collections::BTreeMap<String, u32> = Default::default();
    for f in findings {
        match f.severity {
            mcp_recon_core::Severity::Info => info += 1,
            mcp_recon_core::Severity::Low => low += 1,
            mcp_recon_core::Severity::Medium => medium += 1,
            mcp_recon_core::Severity::High => high += 1,
            mcp_recon_core::Severity::Critical => critical += 1,
        }
        let cat = serde_json::to_value(f.category)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "other".to_string());
        *by_category.entry(cat).or_insert(0) += 1;
    }
    json!({
        "total": findings.len() as u32,
        "by_severity": {
            "info": info, "low": low, "medium": medium, "high": high, "critical": critical
        },
        "by_category": by_category,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_recon_core::{McpServer, Tool, Transport, INVENTORY_SCHEMA};

    fn fixture_inventory() -> McpInventory {
        McpInventory {
            schema: INVENTORY_SCHEMA.to_string(),
            servers: vec![McpServer {
                name: "demo".to_string(),
                transport: Some(Transport::Stdio),
                tools: vec![Tool {
                    name: "fetch_url".to_string(),
                    description: Some("Fetch a URL".to_string()),
                    parameters: None,
                    side_effects: vec![],
                    auth_required: None,
                    rate_limited: None,
                }],
            }],
        }
    }

    #[test]
    fn envelope_has_required_v2_fields() {
        let inv = fixture_inventory();
        let findings = mcp_recon_core::classify(&inv);
        let doc = build(EnvelopeInput {
            scan_id: Uuid::nil(),
            scanned_at: OffsetDateTime::UNIX_EPOCH,
            scanner_name: "mcp-recon",
            scanner_version: "0.0.0-test",
            handle: "npm:demo@0.0.0".to_string(),
            source: ProducerSource::Registry,
            repo_url: Some("https://github.com/example/demo".to_string()),
            name: Some("demo".to_string()),
            inventory: &inv,
            findings,
        })
        .unwrap();

        assert_eq!(doc["schema_version"], "capframe.findings.v2");
        assert_eq!(doc["scan_id"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(doc["server"]["handle"], "npm:demo@0.0.0");
        assert_eq!(doc["server"]["kind"], "mcp_server");
        assert_eq!(doc["server"]["source"], "registry");
        assert_eq!(doc["server"]["transport"], "stdio");
        assert_eq!(doc["scanner"]["name"], "mcp-recon");
        assert!(doc["findings"].is_array());
        assert!(doc["summary"]["total"].is_number());
    }

    #[test]
    fn summary_aggregates_severities() {
        use mcp_recon_core::{Category, Finding, Mappings, Severity};
        let findings = vec![
            Finding {
                id: "a".into(),
                severity: Severity::Critical,
                category: Category::ExcessiveAgency,
                title: "x".into(),
                description: None,
                tool: None,
                remediation: None,
                mappings: Mappings::default(),
            },
            Finding {
                id: "b".into(),
                severity: Severity::High,
                category: Category::ExcessiveAgency,
                title: "y".into(),
                description: None,
                tool: None,
                remediation: None,
                mappings: Mappings::default(),
            },
            Finding {
                id: "c".into(),
                severity: Severity::High,
                category: Category::IndirectInjection,
                title: "z".into(),
                description: None,
                tool: None,
                remediation: None,
                mappings: Mappings::default(),
            },
        ];
        let v = summarize(&findings);
        assert_eq!(v["total"], 3);
        assert_eq!(v["by_severity"]["critical"], 1);
        assert_eq!(v["by_severity"]["high"], 2);
        assert_eq!(v["by_category"]["excessive_agency"], 2);
        assert_eq!(v["by_category"]["indirect_injection"], 1);
    }
}
