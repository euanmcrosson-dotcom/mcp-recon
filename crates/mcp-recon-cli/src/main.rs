//! `mcp-recon` CLI — emits a `findings.v1` JSON document describing the
//! tool surface of the target MCP server. Designed to be dispatched by the
//! Capframe umbrella CLI (`capframe find`). The classifier/fuzzer logic
//! lives in `mcp-recon-core`; this binary is the wire-format glue.

use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Parser, Debug)]
#[command(
    name = "mcp-recon",
    version,
    about = "Discover MCP tool surface and emit findings.v1 JSON"
)]
struct Cli {
    /// Path to the MCP server configuration or transport spec.
    #[arg(long)]
    target: PathBuf,

    /// Output file (default: capframe.findings.json).
    #[arg(long, default_value = "capframe.findings.json")]
    out: PathBuf,

    /// Pretty-print the emitted JSON.
    #[arg(long)]
    pretty: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let findings = build_findings(&cli.target)?;
    let json = if cli.pretty {
        serde_json::to_string_pretty(&findings)?
    } else {
        serde_json::to_string(&findings)?
    };
    fs::write(&cli.out, json).with_context(|| format!("write {}", cli.out.display()))?;
    eprintln!("mcp-recon: wrote {}", cli.out.display());
    Ok(())
}

fn build_findings(target: &Path) -> Result<serde_json::Value> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("format current time")?;
    let exists = target.exists();
    let (total, severity_info, findings_arr) = if exists {
        (0u32, 0u32, serde_json::json!([]))
    } else {
        (
            1u32,
            1u32,
            serde_json::json!([{
                "id": "f-target-not-found",
                "severity": "info",
                "category": "other",
                "title": "Target file not found",
                "description": format!("Could not read target spec at {}", target.display()),
                "remediation": "Pass a valid MCP server config path with --target."
            }]),
        )
    };
    Ok(serde_json::json!({
        "schema_version": "capframe.findings.v1",
        "scanned_at": now,
        "scanner": { "name": "mcp-recon", "version": env!("CARGO_PKG_VERSION") },
        "target": {
            "kind": "mcp_server",
            "path": target.display().to_string()
        },
        "tools": [],
        "findings": findings_arr,
        "summary": {
            "total": total,
            "by_severity": {
                "info": severity_info,
                "low": 0, "medium": 0, "high": 0, "critical": 0
            }
        }
    }))
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
        assert_eq!(v["findings"][0]["id"], "f-target-not-found");
    }

    #[test]
    fn existing_target_yields_empty_findings() {
        let here = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let v = build_findings(&here).unwrap();
        assert_eq!(v["summary"]["total"], 0);
        assert!(v["findings"].as_array().unwrap().is_empty());
    }
}
