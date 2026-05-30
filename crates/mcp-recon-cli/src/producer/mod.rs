//! Registry producer — read a corpus of MCP server handles (npm + PyPI),
//! fetch each one's tool surface from package metadata + README + optional
//! sidecar schemas, classify, and emit one `findings.v2` document per
//! server into an output directory.
//!
//! This is the **static-manifest path** of the Capframe leaderboard
//! pipeline: zero code execution, broad reach (every published MCP
//! package), lower fidelity (rules R3/R5/R6/R7 fire on name/description;
//! R1/R2/R4 need parameter schema and may miss).
//!
//! See the architecture spec in feat/findings-v2-leaderboard for the
//! end-to-end flow.

pub mod corpus;
pub mod envelope;
pub mod http;
pub mod npm;
pub mod pypi;
pub mod readme;
pub mod sandbox;

use anyhow::{Context, Result};
use mcp_recon_core::{classify, McpInventory, McpServer, INVENTORY_SCHEMA};
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

use corpus::CorpusEntry;
use envelope::ProducerSource;
use sandbox::SandboxConfig;

/// Drive the producer end-to-end for a corpus file. For each entry:
/// 1. Fetch the package's tool surface from the appropriate registry.
/// 2. Wrap it in an McpInventory.
/// 3. Run the classifier.
/// 4. Write `<out_dir>/<handle-slug>.findings.v2.json`.
///
/// Returns the number of entries successfully processed (failures are
/// logged but don't abort the corpus walk — one bad package shouldn't
/// take out a 1000-server scan).
pub fn run_registry(
    corpus_path: &Path,
    out_dir: &Path,
    pretty: bool,
    limit: Option<usize>,
) -> Result<usize> {
    let body = fs::read_to_string(corpus_path)
        .with_context(|| format!("read corpus {}", corpus_path.display()))?;
    let entries =
        corpus::parse(&body).with_context(|| format!("parse corpus {}", corpus_path.display()))?;

    fs::create_dir_all(out_dir).with_context(|| format!("create out dir {}", out_dir.display()))?;

    let take = limit.unwrap_or(entries.len()).min(entries.len());
    let mut ok = 0_usize;
    for entry in entries.into_iter().take(take) {
        match produce_one(&entry, out_dir, pretty) {
            Ok(path) => {
                eprintln!("[ok] {} -> {}", entry.handle, path.display());
                ok += 1;
            }
            Err(err) => {
                eprintln!("[err] {} -- {err:#}", entry.handle);
            }
        }
    }
    Ok(ok)
}

fn produce_one(entry: &CorpusEntry, out_dir: &Path, pretty: bool) -> Result<PathBuf> {
    let kind = corpus::ParsedHandle::from_handle(&entry.handle)?;
    let (server, source) = match kind {
        corpus::ParsedHandle::Npm { name, version } => (
            npm::fetch_server(&name, &version)?,
            ProducerSource::Registry,
        ),
        corpus::ParsedHandle::Pypi { name, version } => (
            pypi::fetch_server(&name, &version)?,
            ProducerSource::Registry,
        ),
        // HTTP handles share the registry-walker command surface (one
        // corpus file feeds both) but emit a different source label so
        // the leaderboard can tell readers which path scanned them.
        corpus::ParsedHandle::Http { url } => {
            (http::fetch_server(&url)?, ProducerSource::Http)
        }
    };
    write_findings(entry, server, source, out_dir, pretty)
}

/// Drive the sandbox producer end-to-end. Same shape as `run_registry` but
/// each entry is executed inside an ephemeral Docker container so we capture
/// the live `tools/list` (including parameter schemas → R1/R2/R4 signal).
///
/// Today the sandbox handles **npm packages only**; PyPI entries are skipped
/// with a notice and counted as failures.
pub fn run_sandbox(
    corpus_path: &Path,
    out_dir: &Path,
    pretty: bool,
    limit: Option<usize>,
    config: &SandboxConfig,
) -> Result<usize> {
    let body = fs::read_to_string(corpus_path)
        .with_context(|| format!("read corpus {}", corpus_path.display()))?;
    let entries =
        corpus::parse(&body).with_context(|| format!("parse corpus {}", corpus_path.display()))?;
    fs::create_dir_all(out_dir).with_context(|| format!("create out dir {}", out_dir.display()))?;

    let take = limit.unwrap_or(entries.len()).min(entries.len());
    let mut ok = 0_usize;
    for entry in entries.into_iter().take(take) {
        match produce_one_sandbox(&entry, out_dir, pretty, config) {
            Ok(path) => {
                eprintln!("[sandbox/ok] {} -> {}", entry.handle, path.display());
                ok += 1;
            }
            Err(err) => {
                eprintln!("[sandbox/err] {} -- {err:#}", entry.handle);
            }
        }
    }
    Ok(ok)
}

fn produce_one_sandbox(
    entry: &CorpusEntry,
    out_dir: &Path,
    pretty: bool,
    config: &SandboxConfig,
) -> Result<PathBuf> {
    let kind = corpus::ParsedHandle::from_handle(&entry.handle)?;
    let overrides = entry.sandbox.as_ref();
    let server: McpServer = match kind {
        corpus::ParsedHandle::Npm { name, version } => {
            sandbox::fetch_server_npm(&name, &version, overrides, config)?
        }
        corpus::ParsedHandle::Pypi { name, version } => {
            sandbox::fetch_server_pypi(&name, &version, overrides, config)?
        }
        corpus::ParsedHandle::Http { .. } => {
            anyhow::bail!(
                "sandbox producer does not handle HTTP endpoints (they're already live; use `producer registry`)"
            );
        }
    };
    write_findings(entry, server, ProducerSource::Sandbox, out_dir, pretty)
}

fn write_findings(
    entry: &CorpusEntry,
    server: McpServer,
    source: ProducerSource,
    out_dir: &Path,
    pretty: bool,
) -> Result<PathBuf> {
    let inventory = McpInventory {
        schema: INVENTORY_SCHEMA.to_string(),
        servers: vec![server],
    };
    let findings = classify(&inventory);

    let doc = envelope::build(envelope::EnvelopeInput {
        scan_id: Uuid::new_v4(),
        scanned_at: OffsetDateTime::now_utc(),
        scanner_name: "mcp-recon",
        scanner_version: env!("CARGO_PKG_VERSION"),
        handle: entry.handle.clone(),
        source,
        repo_url: entry.repo_url.clone(),
        name: entry.name.clone(),
        inventory: &inventory,
        findings,
    })?;

    let slug = slugify_handle(&entry.handle);
    let out_path = out_dir.join(format!("{slug}.findings.v2.json"));
    let out = if pretty {
        serde_json::to_string_pretty(&doc)?
    } else {
        serde_json::to_string(&doc)?
    };
    fs::write(&out_path, out).with_context(|| format!("write {}", out_path.display()))?;
    Ok(out_path)
}

/// Turn `npm:@scope/name@1.2.3` into a filename-safe slug.
fn slugify_handle(handle: &str) -> String {
    handle
        .chars()
        .map(|c| match c {
            ':' => '_',
            '/' => '-',
            '@' => '_',
            '.' => '_',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_handles_scoped_npm() {
        assert_eq!(
            slugify_handle("npm:@modelcontextprotocol/server-everything@0.1.0"),
            "npm__modelcontextprotocol-server-everything_0_1_0"
        );
    }

    #[test]
    fn slugify_handles_pypi() {
        assert_eq!(
            slugify_handle("pypi:mcp-server-git@0.6.0"),
            "pypi_mcp-server-git_0_6_0"
        );
    }

    /// End-to-end smoke test that exercises the full pipeline:
    /// corpus → real npm + PyPI fetchers → classifier → v2 envelope.
    /// Hits live registries; gated behind `--ignored` so `cargo test`
    /// stays hermetic. Run with: `cargo test -- --ignored producer`.
    #[test]
    #[ignore = "hits live npm + PyPI registries; opt-in via --ignored"]
    fn run_registry_emits_one_file_per_entry() {
        let dir = std::env::temp_dir().join(format!("mcp-recon-producer-test-{}", Uuid::new_v4()));
        let corpus_path = dir.join("corpus.json");
        let out_dir = dir.join("out");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &corpus_path,
            r#"[
                {"handle": "npm:@mcp/example@1.0.0", "repo_url": "https://github.com/x/y"},
                {"handle": "pypi:mcp-foo@2.3.4"}
            ]"#,
        )
        .unwrap();

        let ok = run_registry(&corpus_path, &out_dir, true, None).unwrap();
        assert_eq!(ok, 2);

        let mut files: Vec<_> = fs::read_dir(&out_dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        files.sort();
        assert_eq!(files.len(), 2);

        for f in &files {
            let body = fs::read_to_string(f).unwrap();
            let doc: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(doc["schema_version"], "capframe.findings.v2");
            assert!(doc["server"]["handle"].is_string());
            assert_eq!(doc["server"]["source"], "registry");
            assert_eq!(doc["server"]["kind"], "mcp_server");
            assert!(doc["findings"].is_array());
            assert!(doc["summary"]["total"].is_number());
        }

        fs::remove_dir_all(&dir).ok();
    }
}
