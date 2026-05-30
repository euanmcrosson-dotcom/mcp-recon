//! Corpus = the list of MCP servers to scan. Today, parsed from a JSON
//! array of [`CorpusEntry`] objects. v1 corpus is hand-curated /
//! scraped from `awesome-mcp-servers`; v2 will pull from npm + PyPI
//! registry queries automatically.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorpusEntry {
    /// Stable globally-unique handle. Format:
    ///   - `npm:<package>@<version>`   (e.g. `npm:@modelcontextprotocol/server-everything@0.1.0`)
    ///   - `pypi:<package>@<version>`  (e.g. `pypi:mcp-server-git@0.6.0`)
    pub handle: String,
    /// GitHub/GitLab URL if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    /// Optional human-readable name. Defaults to the package name part of `handle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional sandbox overrides — extra argv + env vars to satisfy
    /// servers that demand credentials at startup (most do, even just
    /// to advertise tools/list). Static-manifest producer ignores this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxOverrides>,
}

/// Per-entry overrides applied only by the sandbox producer.
///
/// **Use dummy / test values only** — the corpus file is checked into
/// a public repo. Real credentials must come from GHA secrets and be
/// merged at runtime; this struct exists to let public servers boot
/// far enough to advertise `tools/list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxOverrides {
    /// Extra positional arguments appended after the server binary
    /// invocation (e.g. a dummy database URL).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Environment variables forwarded to `docker run -e KEY=VALUE`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// Parse a corpus JSON document. Accepts either a bare array or
/// `{ "entries": [...] }` for forward compat with corpus metadata.
pub fn parse(body: &str) -> Result<Vec<CorpusEntry>> {
    let v: serde_json::Value = serde_json::from_str(body)?;
    if let Some(arr) = v.as_array() {
        return Ok(serde_json::from_value(serde_json::Value::Array(
            arr.clone(),
        ))?);
    }
    if let Some(obj) = v.as_object() {
        if let Some(entries) = obj.get("entries") {
            return Ok(serde_json::from_value(entries.clone())?);
        }
    }
    Err(anyhow!(
        "corpus must be a JSON array or an object with an `entries` key"
    ))
}

/// Discriminated handle — the producer dispatches on this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedHandle {
    Npm { name: String, version: String },
    Pypi { name: String, version: String },
    /// Live HTTP MCP endpoint. The handle is the URL itself
    /// (e.g. `https://example.com/mcp`); no version field, since
    /// HTTP servers are versioned by the operator behind the URL.
    Http { url: String },
}

impl ParsedHandle {
    /// Parse a handle.
    ///
    /// Two forms are supported:
    /// - **Registry handle:** `<registry>:<name>@<version>`. The name
    ///   may contain `@` (e.g. `@scope/pkg`); we split on the LAST `@`
    ///   to find the version boundary.
    /// - **HTTP handle:** a bare `http://` or `https://` URL. No
    ///   version segment; the URL is the identity.
    pub fn from_handle(handle: &str) -> Result<Self> {
        if handle.starts_with("http://") || handle.starts_with("https://") {
            return Ok(ParsedHandle::Http {
                url: handle.to_string(),
            });
        }
        let (prefix, rest) = handle.split_once(':').ok_or_else(|| {
            anyhow!("handle '{handle}' missing registry prefix (expected `<registry>:<name>@<version>` or a `https://` URL)")
        })?;
        let at = rest
            .rfind('@')
            .ok_or_else(|| anyhow!("handle '{handle}' missing `@<version>`"))?;
        let (name, after_at) = rest.split_at(at);
        if name.is_empty() {
            bail!("handle '{handle}' has empty package name");
        }
        let version = after_at
            .strip_prefix('@')
            .ok_or_else(|| anyhow!("handle '{handle}': version missing after '@'"))?
            .to_string();
        if version.is_empty() {
            bail!("handle '{handle}' has empty version");
        }
        match prefix {
            "npm" => Ok(ParsedHandle::Npm {
                name: name.to_string(),
                version,
            }),
            "pypi" => Ok(ParsedHandle::Pypi {
                name: name.to_string(),
                version,
            }),
            other => Err(anyhow!(
                "handle '{handle}': unsupported registry prefix '{other}' (want `npm`, `pypi`, or a `https://` URL)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_array_form() {
        let body = r#"[
            {"handle": "npm:@mcp/example@1.0.0"},
            {"handle": "pypi:mcp-foo@2.3.4", "repo_url": "https://github.com/x/y"}
        ]"#;
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].handle, "npm:@mcp/example@1.0.0");
        assert_eq!(
            entries[1].repo_url.as_deref(),
            Some("https://github.com/x/y")
        );
    }

    #[test]
    fn parses_entries_object_form() {
        let body = r#"{"entries": [{"handle": "npm:foo@0.0.0"}]}"#;
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("\"not an array\"").is_err());
    }

    #[test]
    fn parses_npm_scoped() {
        let h =
            ParsedHandle::from_handle("npm:@modelcontextprotocol/server-everything@0.1.0").unwrap();
        assert_eq!(
            h,
            ParsedHandle::Npm {
                name: "@modelcontextprotocol/server-everything".into(),
                version: "0.1.0".into(),
            }
        );
    }

    #[test]
    fn parses_npm_unscoped() {
        let h = ParsedHandle::from_handle("npm:mcp-server-foo@2.3.4").unwrap();
        assert_eq!(
            h,
            ParsedHandle::Npm {
                name: "mcp-server-foo".into(),
                version: "2.3.4".into(),
            }
        );
    }

    #[test]
    fn parses_pypi() {
        let h = ParsedHandle::from_handle("pypi:mcp-server-git@0.6.0").unwrap();
        assert_eq!(
            h,
            ParsedHandle::Pypi {
                name: "mcp-server-git".into(),
                version: "0.6.0".into(),
            }
        );
    }

    #[test]
    fn parses_http_url() {
        let h = ParsedHandle::from_handle("https://example.com/mcp").unwrap();
        assert_eq!(
            h,
            ParsedHandle::Http {
                url: "https://example.com/mcp".into(),
            }
        );
    }

    #[test]
    fn parses_plain_http_url_too() {
        let h = ParsedHandle::from_handle("http://localhost:8080/mcp").unwrap();
        assert_eq!(
            h,
            ParsedHandle::Http {
                url: "http://localhost:8080/mcp".into(),
            }
        );
    }

    #[test]
    fn rejects_unknown_registry() {
        assert!(ParsedHandle::from_handle("crates:foo@1.0.0").is_err());
    }

    #[test]
    fn rejects_missing_version() {
        assert!(ParsedHandle::from_handle("npm:foo").is_err());
    }

    #[test]
    fn rejects_empty_name() {
        assert!(ParsedHandle::from_handle("npm:@1.0.0").is_err());
    }
}
