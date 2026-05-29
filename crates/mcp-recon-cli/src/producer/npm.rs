//! Fetch an MCP server's tool surface from the npm registry.
//!
//! **Scaffold status:** returns a minimal McpServer that exercises the
//! envelope + classifier without making network calls. The real fetcher
//! (which reads package.json + README + optional `mcp-tools.json`
//! sidecars from `https://registry.npmjs.org/<name>/<version>`) lands in
//! a follow-up commit. Marked clearly with `// TODO(producer-npm-fetch)`
//! so the next session can grep + wire.

use anyhow::Result;
use mcp_recon_core::{McpServer, Transport};

/// Fetch the tool surface for a given npm package + version.
///
/// Currently returns a placeholder server with no declared tools so the
/// classifier sees an empty tool surface (0 findings). This is the
/// honest result for "no fetcher yet" — better than synthesizing fake
/// tools that would pollute the leaderboard with bogus scores.
pub fn fetch_server(name: &str, _version: &str) -> Result<McpServer> {
    // TODO(producer-npm-fetch): GET https://registry.npmjs.org/<name>/<version>
    // - Parse package.json for `mcp` keyword + `bin` entrypoint
    // - Fetch README, look for a tools table or `mcp-tools.json` sidecar
    // - Try `<bin>.mcp.json` manifest convention (if/when one emerges)
    // - Synthesize Tool entries with name/description from README headings
    //
    // Until then this returns an empty inventory so the pipeline runs
    // end-to-end and emits valid findings.v2 envelopes.
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
        let s = fetch_server("@modelcontextprotocol/server-everything", "0.1.0").unwrap();
        assert_eq!(s.name, "@modelcontextprotocol/server-everything");
        assert_eq!(s.transport, Some(Transport::Stdio));
        assert!(s.tools.is_empty());
    }
}
