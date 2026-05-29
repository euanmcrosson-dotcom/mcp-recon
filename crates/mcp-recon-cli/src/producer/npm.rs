//! Fetch an MCP server's tool surface from the npm registry.
//!
//! Strategy (no code execution):
//!   1. GET `https://registry.npmjs.org/<name>/<version>` for the
//!      package manifest.
//!   2. Verify it self-identifies as an MCP package via `keywords`
//!      (`mcp` / `model-context-protocol`) — junk packages get
//!      skipped instead of polluting the leaderboard.
//!   3. Synthesize one [`Tool`] entry per `bin` key (the executable
//!      surface), using `name = <bin key>`, `description = <package
//!      description>`. That's enough for the classifier's
//!      name/description-based rules (R3/R5/R6/R7) to fire.
//!
//! If no `bin` is present, fall back to a single tool whose name is
//! the package's last path segment and whose description is the
//! package description.

use anyhow::{anyhow, Context, Result};
use mcp_recon_core::{McpServer, Tool, Transport};
use serde_json::Value;
use std::time::Duration;

use super::readme;

const NPM_REGISTRY: &str = "https://registry.npmjs.org";
const MCP_KEYWORDS: &[&str] = &[
    "mcp",
    "model-context-protocol",
    "modelcontextprotocol",
    "mcp-server",
];
const HTTP_TIMEOUT_SECS: u64 = 15;

/// Fetch + classify-ready inventory for a single npm package version.
/// Errors propagate; the producer logs and moves on so a single bad
/// package never tanks the corpus walk.
///
/// Two HTTP requests:
///   1. Version-specific manifest — canonical metadata (keywords, bin).
///   2. Package-level manifest — for the README, which the version
///      endpoint omits. Best-effort; on failure we fall back to
///      bin-based tool synthesis.
pub fn fetch_server(name: &str, version: &str) -> Result<McpServer> {
    let url = manifest_url(name, version);
    let body = http_get(&url).with_context(|| format!("GET {url}"))?;
    let manifest: Value = serde_json::from_str(&body)
        .with_context(|| format!("parse npm manifest for {name}@{version}"))?;
    let readme_md = fetch_package_readme(name).unwrap_or_default();
    parse_with_readme(&manifest, &readme_md, name)
}

fn manifest_url(name: &str, version: &str) -> String {
    // npm registry quirk: scoped packages have URL-encoded slashes
    // (`%2F`), but its own router also accepts the raw slash. We use
    // raw because ureq doesn't auto-encode and the registry is fine
    // with both forms.
    format!("{NPM_REGISTRY}/{name}/{version}")
}

fn package_url(name: &str) -> String {
    format!("{NPM_REGISTRY}/{name}")
}

fn fetch_package_readme(name: &str) -> Result<String> {
    let url = package_url(name);
    let body = http_get(&url).with_context(|| format!("GET {url}"))?;
    let manifest: Value = serde_json::from_str(&body)
        .with_context(|| format!("parse package manifest for {name}"))?;
    Ok(manifest
        .get("readme")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
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
        Err(ureq::Error::Status(404, _)) => Err(anyhow!("npm registry returned 404 for {url}")),
        Err(e) => Err(anyhow!("npm registry GET failed: {e}")),
    }
}

/// Build an McpServer from manifest + optional README. README-extracted
/// tools win when non-empty (richer per-tool descriptions for the
/// classifier); otherwise we fall back to bin-based synthesis.
pub fn parse_with_readme(
    manifest: &Value,
    readme_md: &str,
    package_name: &str,
) -> Result<McpServer> {
    let description = manifest
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string);

    if !looks_like_mcp_package(manifest) {
        return Err(anyhow!(
            "{package_name}: package does not declare an MCP keyword (skipped)",
        ));
    }

    // Preferred: README-extracted tool surface.
    let from_readme = readme::extract_tools(readme_md);
    if !from_readme.is_empty() {
        let tools = from_readme
            .into_iter()
            .map(|t| Tool {
                name: t.name,
                description: t.description.or_else(|| description.clone()),
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

    // Fallback: bin-based synthesis.
    let bins = collect_bin_names(manifest);
    let tools = if bins.is_empty() {
        let leaf = leaf_name(package_name);
        vec![Tool {
            name: leaf,
            description: description.clone(),
            parameters: None,
            side_effects: vec![],
            auth_required: None,
            rate_limited: None,
        }]
    } else {
        bins.into_iter()
            .map(|bin| Tool {
                name: bin,
                description: description.clone(),
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

fn looks_like_mcp_package(manifest: &Value) -> bool {
    // Strong signal: explicit MCP keyword in `keywords`.
    if let Some(arr) = manifest.get("keywords").and_then(Value::as_array) {
        if arr
            .iter()
            .filter_map(Value::as_str)
            .any(|s| MCP_KEYWORDS.iter().any(|m| s.eq_ignore_ascii_case(m)))
        {
            return true;
        }
    }
    // Fallback: name-based MCP signature. Caught by E2E:
    // `@modelcontextprotocol/server-everything` has NO keywords field
    // but is obviously the reference MCP server. We accept:
    //   - `@modelcontextprotocol/*` scope
    //   - any package whose name contains `mcp-server` or starts with `mcp-`
    if let Some(name) = manifest.get("name").and_then(Value::as_str) {
        let lower = name.to_lowercase();
        if lower.starts_with("@modelcontextprotocol/")
            || lower.starts_with("@modelcontext/")
            || lower.contains("mcp-server")
            || lower.starts_with("mcp-")
        {
            return true;
        }
    }
    false
}

fn collect_bin_names(manifest: &Value) -> Vec<String> {
    let bin = match manifest.get("bin") {
        Some(b) => b,
        None => return Vec::new(),
    };
    // `bin` can be a string (single bin, named after the package) OR
    // an object { name: path }.
    if bin.is_string() {
        // Use the package name's leaf as the bin name.
        let leaf = manifest
            .get("name")
            .and_then(Value::as_str)
            .map(leaf_name)
            .unwrap_or_else(|| "bin".to_string());
        return vec![leaf];
    }
    if let Some(obj) = bin.as_object() {
        let mut names: Vec<String> = obj.keys().cloned().collect();
        names.sort();
        return names;
    }
    Vec::new()
}

fn leaf_name(package_name: &str) -> String {
    package_name
        .rsplit('/')
        .next()
        .unwrap_or(package_name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mcp_manifest_with_bin_object() -> Value {
        json!({
            "name": "@modelcontextprotocol/server-everything",
            "version": "0.1.0",
            "description": "Reference MCP server exposing fetch + filesystem + git tools",
            "keywords": ["mcp", "model-context-protocol"],
            "bin": {
                "mcp-everything": "dist/index.js",
                "mcp-fetch":      "dist/fetch.js"
            }
        })
    }

    fn mcp_manifest_with_string_bin() -> Value {
        json!({
            "name": "mcp-server-foo",
            "version": "1.0.0",
            "description": "Foo tools for MCP",
            "keywords": ["mcp-server"],
            "bin": "dist/cli.js"
        })
    }

    fn non_mcp_manifest() -> Value {
        json!({
            "name": "leftpad",
            "version": "1.0.0",
            "description": "pads a string",
            "keywords": ["string", "util"]
        })
    }

    fn mcp_manifest_no_bin() -> Value {
        json!({
            "name": "@scope/library-only",
            "version": "0.2.0",
            "description": "MCP helpers, no executable bin",
            "keywords": ["mcp"]
        })
    }

    #[test]
    fn parses_bin_object_into_one_tool_per_bin_key() {
        let m = mcp_manifest_with_bin_object();
        let server = parse_with_readme(&m, "", "@modelcontextprotocol/server-everything").unwrap();
        assert_eq!(server.name, "@modelcontextprotocol/server-everything");
        assert_eq!(server.tools.len(), 2);
        let names: Vec<&str> = server.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["mcp-everything", "mcp-fetch"]);
        // Description from package flows down to each tool — the
        // classifier's R5/R6 rules read this.
        assert_eq!(
            server.tools[0].description.as_deref(),
            Some("Reference MCP server exposing fetch + filesystem + git tools")
        );
    }

    #[test]
    fn parses_string_bin_using_leaf_name() {
        let m = mcp_manifest_with_string_bin();
        let server = parse_with_readme(&m, "", "mcp-server-foo").unwrap();
        assert_eq!(server.tools.len(), 1);
        assert_eq!(server.tools[0].name, "mcp-server-foo");
    }

    #[test]
    fn rejects_non_mcp_package() {
        let m = non_mcp_manifest();
        let err = parse_with_readme(&m, "", "leftpad").unwrap_err();
        assert!(err.to_string().contains("does not declare an MCP keyword"));
    }

    #[test]
    fn synthesizes_fallback_tool_when_no_bin() {
        let m = mcp_manifest_no_bin();
        let server = parse_with_readme(&m, "", "@scope/library-only").unwrap();
        assert_eq!(server.tools.len(), 1);
        assert_eq!(server.tools[0].name, "library-only");
    }

    #[test]
    fn keyword_match_is_case_insensitive() {
        let m = json!({
            "name": "x",
            "description": "y",
            "keywords": ["MCP-Server"]
        });
        let server = parse_with_readme(&m, "", "x").unwrap();
        assert_eq!(server.tools.len(), 1);
    }

    #[test]
    fn name_based_fallback_accepts_modelcontextprotocol_scope() {
        // Real-world: @modelcontextprotocol/server-everything ships
        // with NO keywords field. Should still be on the leaderboard.
        let m = json!({
            "name": "@modelcontextprotocol/server-everything",
            "description": "reference everything server",
            "bin": { "mcp-server-everything": "dist/index.js" }
        });
        let server = parse_with_readme(&m, "", "@modelcontextprotocol/server-everything").unwrap();
        assert_eq!(server.tools.len(), 1);
        assert_eq!(server.tools[0].name, "mcp-server-everything");
    }

    #[test]
    fn name_based_fallback_accepts_mcp_server_prefix() {
        let m = json!({
            "name": "mcp-server-anything",
            "description": "anything"
        });
        assert!(parse_with_readme(&m, "", "mcp-server-anything").is_ok());
    }

    #[test]
    fn name_based_fallback_rejects_unrelated_packages() {
        let m = json!({
            "name": "left-pad",
            "description": "pads strings"
        });
        assert!(parse_with_readme(&m, "", "left-pad").is_err());
    }

    #[test]
    fn manifest_url_format() {
        assert_eq!(
            manifest_url("@scope/name", "1.2.3"),
            "https://registry.npmjs.org/@scope/name/1.2.3"
        );
    }

    #[test]
    fn leaf_name_handles_scoped_packages() {
        assert_eq!(leaf_name("@scope/foo"), "foo");
        assert_eq!(leaf_name("unscoped"), "unscoped");
    }
}
