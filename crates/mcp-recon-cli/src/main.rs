//! `mcp-recon` CLI — emits a `findings.v1` JSON document describing the
//! tool surface of the target MCP inventory. Designed to be dispatched by
//! the Capframe umbrella CLI (`capframe find`).
//!
//! Input file is an `mcp-recon.inventory.v1` document (see
//! [`mcp_recon_core::McpInventory`]). If parsing fails the CLI still emits a
//! valid `findings.v1.json` envelope with a single informational finding,
//! so downstream tools never see broken output.

mod enumerate;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mcp_recon_core::{classify, Finding, McpInventory, Severity};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Parser, Debug)]
#[command(
    name = "mcp-recon",
    version,
    about = "Discover MCP tool surface and emit findings.v1 JSON"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,

    // ── Legacy top-level flags: `mcp-recon --target <inventory> --out <findings>` ──
    // Kept so `capframe find` (which dispatches this exact shape) is unaffected.
    /// Path to an mcp-recon.inventory.v1 JSON file (classify mode).
    #[arg(long)]
    target: Option<PathBuf>,

    /// Output file for classify mode (default: capframe.findings.json).
    #[arg(long, default_value = "capframe.findings.json")]
    out: PathBuf,

    /// Pretty-print the emitted JSON.
    #[arg(long)]
    pretty: bool,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Live-enumerate MCP servers from a claude_desktop_config.json (stdio
    /// transport): launch each server, handshake, call tools/list, and write an
    /// mcp-recon.inventory.v1 you can then classify.
    Enumerate {
        /// Path to a claude_desktop_config.json (Cursor / Cline configs work too).
        config: PathBuf,
        /// Output inventory path.
        #[arg(long, default_value = "mcp-recon.inventory.json")]
        out: PathBuf,
        /// Pretty-print the emitted inventory.
        #[arg(long)]
        pretty: bool,
        /// Per-server handshake + tools/list timeout, in seconds.
        #[arg(long, default_value_t = 15)]
        timeout_secs: u64,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Some(Cmd::Enumerate {
            config,
            out,
            pretty,
            timeout_secs,
        }) => run_enumerate(&config, &out, pretty, Duration::from_secs(timeout_secs)),
        None => run_classify(cli.target.as_deref(), &cli.out, cli.pretty),
    }
}

fn run_classify(target: Option<&Path>, out: &Path, pretty: bool) -> Result<()> {
    let target = target.context(
        "no input given. Either pass --target <inventory.json> to classify, \
         or use `mcp-recon enumerate <claude_desktop_config.json>` to build one live.",
    )?;
    let findings = build_findings(target)?;
    let json = if pretty {
        serde_json::to_string_pretty(&findings)?
    } else {
        serde_json::to_string(&findings)?
    };
    fs::write(out, json).with_context(|| format!("write {}", out.display()))?;
    eprintln!("mcp-recon: wrote {}", out.display());
    Ok(())
}

fn run_enumerate(config: &Path, out: &Path, pretty: bool, timeout: Duration) -> Result<()> {
    let body = fs::read_to_string(config).with_context(|| format!("read {}", config.display()))?;
    eprintln!("mcp-recon: enumerating MCP servers (stdio)…");
    let inventory = enumerate::enumerate_config(&body, timeout)?;
    let json = if pretty {
        serde_json::to_string_pretty(&inventory)?
    } else {
        serde_json::to_string(&inventory)?
    };
    fs::write(out, json).with_context(|| format!("write {}", out.display()))?;
    let total: usize = inventory.servers.iter().map(|s| s.tools.len()).sum();
    eprintln!(
        "mcp-recon: wrote {} ({} servers, {} tools) — classify with: mcp-recon --target {}",
        out.display(),
        inventory.servers.len(),
        total,
        out.display()
    );
    Ok(())
}

fn build_findings(target: &Path) -> Result<serde_json::Value> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("format current time")?;

    // Try to read + parse the target as an inventory. Any failure becomes a
    // single info-level finding so the CLI never emits broken output.
    let outcome = read_and_classify(target);

    let (tools_json, findings, scan_target_path) = match outcome {
        Ok((inv, findings)) => (
            inventory_tools_to_findings_v1(&inv),
            findings,
            target.display().to_string(),
        ),
        Err(e) => (
            serde_json::Value::Array(vec![]),
            vec![Finding {
                id: "f-target-unreadable".into(),
                severity: Severity::Info,
                category: mcp_recon_core::Category::Other,
                title: "Target inventory could not be read".into(),
                description: Some(format!("{e:#}")),
                tool: None,
                remediation: Some("Pass an mcp-recon.inventory.v1 JSON file via --target.".into()),
                mappings: Default::default(),
            }],
            target.display().to_string(),
        ),
    };

    let severity_counts = count_by_severity(&findings);
    let total: u32 = severity_counts.values().sum();

    Ok(json!({
        "schema_version": "capframe.findings.v1",
        "scanned_at": now,
        "scanner": { "name": "mcp-recon", "version": env!("CARGO_PKG_VERSION") },
        "target": {
            "kind": "mcp_server",
            "path": scan_target_path
        },
        "tools": tools_json,
        "findings": findings,
        "summary": {
            "total": total,
            "by_severity": {
                "info":     severity_counts.get(&Severity::Info).copied().unwrap_or(0),
                "low":      severity_counts.get(&Severity::Low).copied().unwrap_or(0),
                "medium":   severity_counts.get(&Severity::Medium).copied().unwrap_or(0),
                "high":     severity_counts.get(&Severity::High).copied().unwrap_or(0),
                "critical": severity_counts.get(&Severity::Critical).copied().unwrap_or(0),
            }
        }
    }))
}

fn read_and_classify(target: &Path) -> Result<(McpInventory, Vec<Finding>)> {
    let body = fs::read_to_string(target).with_context(|| format!("read {}", target.display()))?;
    let inv: McpInventory = serde_json::from_str(&body)
        .with_context(|| format!("parse {} as mcp-recon.inventory.v1", target.display()))?;
    let findings = classify(&inv);
    Ok((inv, findings))
}

fn inventory_tools_to_findings_v1(inv: &McpInventory) -> serde_json::Value {
    let mut tools = Vec::new();
    for server in &inv.servers {
        for t in &server.tools {
            let mut entry = serde_json::Map::new();
            entry.insert("name".into(), json!(t.name));
            if let Some(d) = &t.description {
                entry.insert("description".into(), json!(d));
            }
            if let Some(p) = &t.parameters {
                entry.insert("parameters".into(), p.clone());
            }
            if !t.side_effects.is_empty() {
                entry.insert("side_effects".into(), json!(t.side_effects));
            }
            if let Some(a) = t.auth_required {
                entry.insert("auth_required".into(), json!(a));
            }
            if let Some(r) = t.rate_limited {
                entry.insert("rate_limited".into(), json!(r));
            }
            tools.push(serde_json::Value::Object(entry));
        }
    }
    serde_json::Value::Array(tools)
}

fn count_by_severity(findings: &[Finding]) -> std::collections::HashMap<Severity, u32> {
    let mut m = std::collections::HashMap::new();
    for f in findings {
        *m.entry(f.severity).or_insert(0) += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_target_yields_info_finding() {
        let nope = std::path::PathBuf::from("/__nope__/does-not-exist.toml");
        let v = build_findings(&nope).unwrap();
        assert_eq!(v["schema_version"], "capframe.findings.v1");
        assert_eq!(v["summary"]["total"], 1);
        assert_eq!(v["summary"]["by_severity"]["info"], 1);
        assert_eq!(v["findings"][0]["id"], "f-target-unreadable");
    }

    #[test]
    fn valid_inventory_yields_real_findings() {
        // An inventory with one tool that triggers R1 (unconstrained string).
        let tmp = std::env::temp_dir().join("mcp-recon-test-inv.json");
        let inv = serde_json::json!({
            "schema": "mcp-recon.inventory.v1",
            "servers": [{
                "name": "test-server",
                "tools": [{
                    "name": "lookup",
                    "description": "Look something up",
                    "parameters": {
                        "type": "object",
                        "properties": { "query": { "type": "string" } }
                    },
                    "side_effects": ["read"],
                    "auth_required": true
                }]
            }]
        });
        fs::write(&tmp, serde_json::to_string(&inv).unwrap()).unwrap();
        let v = build_findings(&tmp).unwrap();
        let _ = fs::remove_file(&tmp);
        assert_eq!(v["summary"]["total"], 1);
        assert_eq!(v["summary"]["by_severity"]["medium"], 1);
        assert_eq!(v["findings"][0]["category"], "unconstrained_input");
        assert_eq!(v["tools"][0]["name"], "lookup");
    }
}
