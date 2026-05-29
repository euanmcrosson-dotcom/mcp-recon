//! Fetch an MCP server's tool surface from PyPI.
//!
//! **Scaffold status:** mirrors [`super::npm`] — returns a minimal
//! McpServer until the real fetcher lands. PyPI's JSON metadata API is
//! at `https://pypi.org/pypi/<name>/<version>/json`.

use anyhow::Result;
use mcp_recon_core::{McpServer, Transport};

pub fn fetch_server(name: &str, _version: &str) -> Result<McpServer> {
    // TODO(producer-pypi-fetch): GET https://pypi.org/pypi/<name>/<version>/json
    // - Parse `info.classifiers` + `info.keywords` for MCP markers
    // - Pull README from `info.description`
    // - Look for `[project.entry-points."mcp.tools"]` in
    //   `urls[].url`-discovered pyproject.toml (if reachable)
    // - Synthesize Tool entries the same way as the npm fetcher
    Ok(McpServer {
        name: name.to_string(),
        transport: Some(Transport::Stdio),
        tools: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_returns_named_server_with_no_tools() {
        let s = fetch_server("mcp-server-git", "0.6.0").unwrap();
        assert_eq!(s.name, "mcp-server-git");
        assert!(s.tools.is_empty());
    }
}
