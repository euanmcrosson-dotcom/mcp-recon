//! `mcp-recon` CLI — emits a `findings.v1` JSON document describing the
//! tool surface of the target MCP inventory. Designed to be dispatched by
//! the Capframe umbrella CLI (`capframe find`).
//!
//! Input file is an `mcp-recon.inventory.v1` document (see
//! [`mcp_recon_core::McpInventory`]). If parsing fails the CLI still emits a
//! valid `findings.v1.json` envelope with a single informational finding,
//! so downstream tools never see broken output.

mod enumerate;
mod mcp_server;
mod producer;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use mcp_recon_core::{
    classify, from_anthropic_tools, from_langchain_tools, from_openai_tools, Finding, McpInventory,
    Severity,
};
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

    /// Turn an inventory's findings into capnagent-ready issuance plans
    /// (`mcp-recon/v0.1/caveats`): a deny/scope recommendation plus caveat-DSL
    /// strings and rule provenance for every authority-relevant tool. The
    /// Find → Bind handoff — feed the output to your capnagent issuer.
    Caveats {
        /// Path to an mcp-recon.inventory.v1 JSON file.
        target: PathBuf,
        /// Output caveats artifact path.
        #[arg(long, default_value = "mcp-recon.caveats.json")]
        out: PathBuf,
        /// Pretty-print the emitted artifact.
        #[arg(long)]
        pretty: bool,
    },

    /// Run mcp-recon as an MCP server itself — newline-delimited JSON-RPC
    /// over stdio. Exposes `classify_inventory` and `caveats` as MCP tools so
    /// any MCP-aware agent can invoke the recon classifier the same way it
    /// invokes any other MCP server.
    McpServer,

    /// Walk a corpus of MCP server handles (npm + PyPI), fetch each one's
    /// tool surface from package metadata, classify, and emit one
    /// `findings.v2.json` per server. Feeds the public Capframe leaderboard.
    Producer {
        #[command(subcommand)]
        kind: ProducerKind,
    },

    /// Convert a non-MCP tool-use payload (Anthropic / OpenAI) into an
    /// `mcp-recon.inventory.v1` document the classifier accepts. Lets teams
    /// already using OpenAI's `tools` or Anthropic's `tool_use` blocks run
    /// mcp-recon against their existing tool surface without rewriting it as
    /// an MCP server first.
    Adapt {
        /// Source format of the input file.
        #[arg(long, value_enum)]
        format: AdaptFormat,
        /// Path to the input JSON file (array of tools or `{ "tools": [...] }`).
        input: PathBuf,
        /// Output inventory path.
        #[arg(long, default_value = "mcp-recon.inventory.json")]
        out: PathBuf,
        /// Name to record on the synthesized server entry. Defaults to the
        /// input file's stem (e.g. `tools.json` → `tools`).
        #[arg(long)]
        server_name: Option<String>,
        /// Pretty-print the emitted inventory.
        #[arg(long)]
        pretty: bool,
    },
}

/// Producer kind — which registry feeds the inventory.
#[derive(Subcommand, Debug)]
enum ProducerKind {
    /// Static-manifest path: fetch tool surface from package metadata on
    /// the npm + PyPI registries (whichever each handle in the corpus
    /// indicates). Zero code execution. Broad reach.
    Registry {
        /// Path to a corpus JSON file (array of `{handle, repo_url?, name?}`).
        corpus: PathBuf,
        /// Output directory; one `<slug>.findings.v2.json` per entry.
        #[arg(long, default_value = "findings/")]
        out_dir: PathBuf,
        /// Pretty-print emitted JSON.
        #[arg(long)]
        pretty: bool,
        /// Process at most N entries (for smoke tests).
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Live-handshake path: run each corpus entry in an ephemeral Docker
    /// container, capture the real `tools/list` (incl. parameter schemas),
    /// classify, and emit one `findings.v2.json` per server. Lights up
    /// R1/R2/R4 rules that the static-manifest path can't see. Requires a
    /// working local `docker` CLI. Supports npm + PyPI.
    Sandbox {
        /// Path to a corpus JSON file (array of `{handle, repo_url?, name?}`).
        corpus: PathBuf,
        /// Output directory; one `<slug>.findings.v2.json` per entry.
        #[arg(long, default_value = "findings/")]
        out_dir: PathBuf,
        /// Pretty-print emitted JSON.
        #[arg(long)]
        pretty: bool,
        /// Process at most N entries (for smoke tests).
        #[arg(long)]
        limit: Option<usize>,
        /// Per-entry wall-clock budget for the container.
        #[arg(long, default_value_t = 120)]
        timeout_secs: u32,
        /// Memory cap passed to `docker run --memory`.
        #[arg(long, default_value_t = 512)]
        memory_mb: u32,
        /// Docker image for npm entries.
        #[arg(long, default_value = "node:20")]
        image: String,
        /// Docker image for PyPI entries.
        #[arg(long, default_value = "python:3.12-slim")]
        image_pypi: String,
    },
}

/// Source format accepted by `mcp-recon adapt`.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum AdaptFormat {
    /// Anthropic tool-use payload — `[{ name, description, input_schema }, ...]`
    /// or `{ "tools": [...] }`.
    Anthropic,
    /// OpenAI function-calling payload — either the current
    /// `[{ type: "function", function: {...} }, ...]` shape or the deprecated
    /// bare `[{ name, description, parameters }, ...]` form.
    Openai,
    /// LangChain `BaseTool` dump — `[{ name, description, args_schema }, ...]`.
    /// Falls back to bare `args` if `args_schema` is missing. For LangChain
    /// stacks that serialize via `convert_to_openai_tool()`, prefer
    /// `--format openai`.
    Langchain,
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
        Some(Cmd::Caveats {
            target,
            out,
            pretty,
        }) => run_caveats(&target, &out, pretty),
        Some(Cmd::McpServer) => mcp_server::run(),
        Some(Cmd::Producer { kind }) => match kind {
            ProducerKind::Registry {
                corpus,
                out_dir,
                pretty,
                limit,
            } => {
                let ok = producer::run_registry(&corpus, &out_dir, pretty, limit)?;
                eprintln!("[producer] {} entries written to {}", ok, out_dir.display());
                Ok(())
            }
            ProducerKind::Sandbox {
                corpus,
                out_dir,
                pretty,
                limit,
                timeout_secs,
                memory_mb,
                image,
                image_pypi,
            } => {
                let config = producer::sandbox::SandboxConfig {
                    timeout_secs,
                    memory_mb,
                    image,
                    image_pypi,
                };
                let ok = producer::run_sandbox(&corpus, &out_dir, pretty, limit, &config)?;
                eprintln!(
                    "[producer/sandbox] {} entries written to {}",
                    ok,
                    out_dir.display()
                );
                Ok(())
            }
        },
        Some(Cmd::Adapt {
            format,
            input,
            out,
            server_name,
            pretty,
        }) => run_adapt(format, &input, &out, server_name.as_deref(), pretty),
        None => run_classify(cli.target.as_deref(), &cli.out, cli.pretty),
    }
}

fn run_adapt(
    format: AdaptFormat,
    input: &Path,
    out: &Path,
    server_name_override: Option<&str>,
    pretty: bool,
) -> Result<()> {
    let body = fs::read_to_string(input).with_context(|| format!("read {}", input.display()))?;
    let payload: serde_json::Value = serde_json::from_str(&body)
        .with_context(|| format!("parse {} as JSON", input.display()))?;

    let server_name = match server_name_override {
        Some(s) => s.to_string(),
        None => input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("tools")
            .to_string(),
    };

    let inventory = match format {
        AdaptFormat::Anthropic => from_anthropic_tools(&payload, &server_name)
            .map_err(|e| anyhow!("adapt anthropic: {e}"))?,
        AdaptFormat::Openai => {
            from_openai_tools(&payload, &server_name).map_err(|e| anyhow!("adapt openai: {e}"))?
        }
        AdaptFormat::Langchain => from_langchain_tools(&payload, &server_name)
            .map_err(|e| anyhow!("adapt langchain: {e}"))?,
    };

    let json = if pretty {
        serde_json::to_string_pretty(&inventory)?
    } else {
        serde_json::to_string(&inventory)?
    };
    fs::write(out, json).with_context(|| format!("write {}", out.display()))?;
    let total: usize = inventory.servers.iter().map(|s| s.tools.len()).sum();
    let format_name = match format {
        AdaptFormat::Anthropic => "anthropic",
        AdaptFormat::Openai => "openai",
        AdaptFormat::Langchain => "langchain",
    };
    eprintln!(
        "mcp-recon: adapted {} {} tool(s) → {} — classify with: mcp-recon --target {}",
        total,
        format_name,
        out.display(),
        out.display()
    );
    Ok(())
}

fn run_caveats(target: &Path, out: &Path, pretty: bool) -> Result<()> {
    let body = fs::read_to_string(target).with_context(|| format!("read {}", target.display()))?;
    let inv: McpInventory = serde_json::from_str(&body)
        .with_context(|| format!("parse {} as mcp-recon.inventory.v1", target.display()))?;
    let artifact = mcp_recon_core::caveats_v1(&inv);
    let json = if pretty {
        serde_json::to_string_pretty(&artifact)?
    } else {
        serde_json::to_string(&artifact)?
    };
    fs::write(out, json).with_context(|| format!("write {}", out.display()))?;
    let denies = artifact
        .plans
        .iter()
        .filter(|p| p.recommend == "deny")
        .count();
    eprintln!(
        "mcp-recon: wrote {} ({} issuance plans, {} deny) — feed to your capnagent issuer",
        out.display(),
        artifact.plans.len(),
        denies
    );
    Ok(())
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

    // Read + parse the target as an inventory. A failure here means we never
    // saw the tool surface, so there is nothing to report on — fail closed.
    //
    // This used to be downgraded to a single info-level finding with an empty
    // `tools` array. That produced a syntactically valid findings file with
    // zero severities and a zero exit code, so a scan that never ran was
    // indistinguishable from a hardened server — and `capframe report`
    // happily rendered it as a clean report.
    let (inv, findings) = read_and_classify(target).with_context(|| {
        format!(
            "scan target {} could not be read as an mcp-recon.inventory.v1 \
             JSON file — refusing to emit findings for a scan that did not run",
            target.display()
        )
    })?;

    let tools_json = inventory_tools_to_findings_v1(&inv);
    let scan_target_path = target.display().to_string();

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

    /// A target we cannot read is a failed scan, not a clean one.
    ///
    /// This previously returned Ok with a single info-level
    /// `f-target-unreadable` finding and `tools: []`. The CLI then wrote that
    /// file and exited 0, so `capframe report` rendered a clean-looking report
    /// for a scan that never ran — a failed scan was indistinguishable from a
    /// hardened server. Fail closed instead.
    #[test]
    fn unreadable_target_fails_closed_rather_than_reporting_clean() {
        let nope = std::path::PathBuf::from("/__nope__/does-not-exist.toml");
        let err = build_findings(&nope).expect_err("unreadable target must be an error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("does-not-exist.toml"),
            "error should name the offending target, got: {msg}"
        );
    }

    /// The failure must not leave a findings file behind — a stale or empty
    /// artifact on disk is what downstream `capframe report` would pick up.
    #[test]
    fn unreadable_target_writes_no_findings_file() {
        let nope = std::path::PathBuf::from("/__nope__/does-not-exist.toml");
        let out = std::env::temp_dir().join("mcp-recon-failclosed-out.json");
        let _ = fs::remove_file(&out);
        assert!(
            run_classify(Some(&nope), &out, false).is_err(),
            "classify must fail on an unreadable target"
        );
        assert!(
            !out.exists(),
            "no findings file may be written for a failed scan"
        );
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
