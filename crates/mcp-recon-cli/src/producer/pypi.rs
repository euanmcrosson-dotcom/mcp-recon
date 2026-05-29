//! Fetch an MCP server's tool surface from PyPI.
//!
//! Mirrors [`super::npm`]:
//!   1. GET `https://pypi.org/pypi/<name>/<version>/json`.
//!   2. Verify MCP self-identification via `info.keywords` (CSV
//!      string, unlike npm's array) or `info.classifiers`.
//!   3. Synthesize tools from `info.entry_points` (CSV string of
//!      `name = module:fn` lines) when present; otherwise fall back
//!      to one tool named after the package leaf.
//!
//! `info.summary` is used as the per-tool description (npm's
//! equivalent of the short `description` field).

use anyhow::{anyhow, Context, Result};
use mcp_recon_core::{McpServer, Tool, Transport};
use serde_json::Value;
use std::time::Duration;

use super::readme;

const PYPI_BASE: &str = "https://pypi.org/pypi";
const MCP_KEYWORDS: &[&str] = &[
    "mcp",
    "model-context-protocol",
    "modelcontextprotocol",
    "mcp-server",
];
const HTTP_TIMEOUT_SECS: u64 = 15;

pub fn fetch_server(name: &str, version: &str) -> Result<McpServer> {
    let url = manifest_url(name, version);
    let body = http_get(&url).with_context(|| format!("GET {url}"))?;
    let manifest: Value = serde_json::from_str(&body)
        .with_context(|| format!("parse PyPI manifest for {name}@{version}"))?;
    parse_manifest(&manifest, name)
}

/// Convenience: extract the README out of a parsed PyPI manifest.
/// PyPI bundles the full long-form description (which is the rendered
/// README in the vast majority of cases) inside `info.description`.
fn readme_from(manifest: &Value) -> &str {
    manifest
        .get("info")
        .and_then(|i| i.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn manifest_url(name: &str, version: &str) -> String {
    format!("{PYPI_BASE}/{name}/{version}/json")
}

fn http_get(url: &str) -> Result<String> {
    let res = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .get(url)
        .set("Accept", "application/json")
        .set(
            "User-Agent",
            concat!("mcp-recon/", env!("CARGO_PKG_VERSION")),
        )
        .call();
    match res {
        Ok(r) => Ok(r.into_string()?),
        Err(ureq::Error::Status(404, _)) => Err(anyhow!("PyPI returned 404 for {url}")),
        Err(e) => Err(anyhow!("PyPI GET failed: {e}")),
    }
}

pub fn parse_manifest(manifest: &Value, package_name: &str) -> Result<McpServer> {
    let info = manifest
        .get("info")
        .ok_or_else(|| anyhow!("{package_name}: PyPI manifest missing `info` block"))?;

    let summary = info
        .get("summary")
        .and_then(Value::as_str)
        .map(str::to_string);

    if !looks_like_mcp_package(info) {
        return Err(anyhow!(
            "{package_name}: PyPI package does not declare an MCP keyword/classifier (skipped)",
        ));
    }

    // Preferred: README-extracted tools (info.description = rendered README).
    let from_readme = readme::extract_tools(readme_from(manifest));
    if !from_readme.is_empty() {
        let tools = from_readme
            .into_iter()
            .map(|t| Tool {
                name: t.name,
                description: t.description.or_else(|| summary.clone()),
                parameters: None,
                side_effects: vec![],
                auth_required: None,
                rate_limited: None,
            })
            .collect();
        return Ok(McpServer {
            name: package_name.to_string(),
            transport: Some(Transport::Stdio),
            tools,
        });
    }

    // Fallback: entry_points-based synthesis.
    let entry_points = parse_entry_points(info);
    let tools = if entry_points.is_empty() {
        vec![Tool {
            name: leaf_name(package_name),
            description: summary.clone(),
            parameters: None,
            side_effects: vec![],
            auth_required: None,
            rate_limited: None,
        }]
    } else {
        entry_points
            .into_iter()
            .map(|name| Tool {
                name,
                description: summary.clone(),
                parameters: None,
                side_effects: vec![],
                auth_required: None,
                rate_limited: None,
            })
            .collect()
    };

    Ok(McpServer {
        name: package_name.to_string(),
        transport: Some(Transport::Stdio),
        tools,
    })
}

fn looks_like_mcp_package(info: &Value) -> bool {
    // Strong signal: explicit MCP keyword (PyPI keywords is a CSV string).
    if let Some(kw) = info.get("keywords").and_then(Value::as_str) {
        let normalized: Vec<&str> = kw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if normalized
            .iter()
            .any(|k| MCP_KEYWORDS.iter().any(|m| k.eq_ignore_ascii_case(m)))
        {
            return true;
        }
    }
    // Classifiers fallback — Trove classifiers often mention MCP.
    if let Some(classifiers) = info.get("classifiers").and_then(Value::as_array) {
        for c in classifiers.iter().filter_map(Value::as_str) {
            let lower = c.to_lowercase();
            if lower.contains("model context protocol")
                || lower.contains("mcp-server")
                || lower.contains("mcp server")
            {
                return true;
            }
        }
    }
    // Name-based fallback (mirrors the npm fix). Caught by E2E against
    // the live PyPI corpus: `mcp-server-sqlite` ships without an MCP
    // keyword or classifier and was being excluded.
    if let Some(name) = info.get("name").and_then(Value::as_str) {
        let lower = name.to_lowercase();
        if lower.starts_with("mcp-server-")
            || lower.starts_with("mcp-server.")
            || lower == "mcp-server"
            || lower.starts_with("mcp-")
        {
            return true;
        }
    }
    false
}

/// `info.entry_points` on PyPI is a string keyed by group name, where
/// each line is `<name> = <module>:<callable>`. We pull *names* from
/// the `console_scripts` and (optionally) any `mcp.tools` group.
///
/// Returns sorted unique names so per-tool output is deterministic.
fn parse_entry_points(info: &Value) -> Vec<String> {
    let raw = match info.get("entry_points").and_then(Value::as_str) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut current_group: Option<String> = None;
    let mut names: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current_group = Some(rest.trim().to_string());
            continue;
        }
        let take_this_group = matches!(
            current_group.as_deref(),
            Some("console_scripts") | Some("mcp.tools") | Some("mcp_servers")
        );
        if !take_this_group {
            continue;
        }
        if let Some((name, _rhs)) = line.split_once('=') {
            let name = name.trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn leaf_name(package_name: &str) -> String {
    // PyPI package names don't have `/` scopes; the name itself is the leaf.
    package_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mcp_pypi_with_entry_points() -> Value {
        json!({
            "info": {
                "name": "mcp-server-git",
                "version": "0.6.0",
                "summary": "Git tools for MCP — fetch, status, log over stdio",
                "keywords": "mcp,git,model-context-protocol",
                "entry_points": "[console_scripts]\nmcp-server-git = mcp_server_git.cli:main\nmcp-git-helper = mcp_server_git.helper:run\n"
            }
        })
    }

    fn mcp_pypi_via_classifier() -> Value {
        json!({
            "info": {
                "name": "modelctx",
                "version": "0.1.0",
                "summary": "Helpers for MCP-server authors",
                "keywords": "",
                "classifiers": [
                    "License :: OSI Approved :: MIT License",
                    "Framework :: Model Context Protocol"
                ]
            }
        })
    }

    fn mcp_pypi_no_entry_points() -> Value {
        json!({
            "info": {
                "name": "mcp-lib-only",
                "version": "0.1.0",
                "summary": "Library-only MCP utilities",
                "keywords": "mcp"
            }
        })
    }

    fn non_mcp_pypi() -> Value {
        json!({
            "info": {
                "name": "requests",
                "version": "2.31.0",
                "summary": "HTTP for humans",
                "keywords": "http,requests"
            }
        })
    }

    #[test]
    fn parses_console_scripts_entry_points() {
        let m = mcp_pypi_with_entry_points();
        let server = parse_manifest(&m, "mcp-server-git").unwrap();
        let names: Vec<&str> = server.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["mcp-git-helper", "mcp-server-git"]);
        assert_eq!(
            server.tools[0].description.as_deref(),
            Some("Git tools for MCP — fetch, status, log over stdio")
        );
    }

    #[test]
    fn detects_mcp_via_classifier_when_keywords_empty() {
        let m = mcp_pypi_via_classifier();
        let server = parse_manifest(&m, "modelctx").unwrap();
        assert_eq!(server.tools.len(), 1);
        assert_eq!(server.tools[0].name, "modelctx");
    }

    #[test]
    fn rejects_non_mcp_package() {
        let m = non_mcp_pypi();
        let err = parse_manifest(&m, "requests").unwrap_err();
        assert!(err.to_string().contains("does not declare an MCP keyword"));
    }

    #[test]
    fn synthesizes_fallback_tool_when_no_entry_points() {
        let m = mcp_pypi_no_entry_points();
        let server = parse_manifest(&m, "mcp-lib-only").unwrap();
        assert_eq!(server.tools.len(), 1);
        assert_eq!(server.tools[0].name, "mcp-lib-only");
    }

    #[test]
    fn keyword_match_is_case_insensitive() {
        let m = json!({
            "info": {
                "name": "x",
                "summary": "y",
                "keywords": "MCP, util"
            }
        });
        let server = parse_manifest(&m, "x").unwrap();
        assert_eq!(server.tools.len(), 1);
    }

    #[test]
    fn name_based_fallback_accepts_mcp_server_prefix() {
        // Real-world: mcp-server-sqlite ships with no MCP keyword or
        // classifier. Should still be on the leaderboard.
        let m = json!({
            "info": {
                "name": "mcp-server-sqlite",
                "summary": "SQLite tools for MCP",
                "keywords": ""
            }
        });
        let server = parse_manifest(&m, "mcp-server-sqlite").unwrap();
        assert_eq!(server.tools.len(), 1);
        assert_eq!(server.tools[0].name, "mcp-server-sqlite");
    }

    #[test]
    fn name_based_fallback_accepts_mcp_prefix() {
        let m = json!({
            "info": {
                "name": "mcp-toolkit",
                "summary": "Generic MCP utilities"
            }
        });
        assert!(parse_manifest(&m, "mcp-toolkit").is_ok());
    }

    #[test]
    fn name_based_fallback_rejects_unrelated_packages() {
        let m = json!({
            "info": {
                "name": "requests",
                "summary": "HTTP for humans"
            }
        });
        assert!(parse_manifest(&m, "requests").is_err());
    }

    #[test]
    fn manifest_url_format() {
        assert_eq!(
            manifest_url("mcp-server-git", "0.6.0"),
            "https://pypi.org/pypi/mcp-server-git/0.6.0/json"
        );
    }

    #[test]
    fn missing_info_block_errors() {
        let m = json!({});
        let err = parse_manifest(&m, "x").unwrap_err();
        assert!(err.to_string().contains("missing `info` block"));
    }

    #[test]
    fn ignores_non_relevant_entry_point_groups() {
        let m = json!({
            "info": {
                "name": "x",
                "summary": "s",
                "keywords": "mcp",
                "entry_points": "[gui_scripts]\nx-gui = x:gui\n"
            }
        });
        let server = parse_manifest(&m, "x").unwrap();
        // gui_scripts isn't a group we care about → fallback to leaf.
        assert_eq!(server.tools.len(), 1);
        assert_eq!(server.tools[0].name, "x");
    }
}
