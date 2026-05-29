//! Sandbox producer — the high-fidelity path of the Capframe leaderboard
//! pipeline.
//!
//! **Scaffold status:** this module declares the public surface
//! (`SandboxConfig`, `fetch_server`) and the orchestration shape
//! [`produce_sandboxed`]. The actual provider integration (Vercel
//! Sandbox / Firecracker microVMs) is its own multi-session build —
//! the entry points return a clear "not implemented" with a pointer to
//! the design doc so callers fail loudly instead of silently emitting
//! empty inventories.
//!
//! See `docs/SANDBOX-PRODUCER.md` for the architecture: per-package
//! ephemeral microVM → `npm install` / `pip install` → spawn stdio
//! server → MCP `initialize` + `tools/list` handshake → capture tools →
//! teardown.
//!
//! Why this lives behind a separate producer rather than feature-flagged
//! on top of [`super::npm`] / [`super::pypi`]:
//!
//!   - Different cost profile (per-scan dollars, not per-scan
//!     bandwidth)
//!   - Different concurrency story (sandbox provider rate limits + VM
//!     warmup time)
//!   - Different failure modes (sandbox provisioning timeout / OOM)
//!   - Different invocation cadence (probably weekly, not daily — too
//!     expensive for a 1000-server corpus every 24 h)
//!
//! The classifier and findings.v2 envelope are unchanged: the sandbox
//! producer emits the same `McpServer` shape as the registry producer,
//! just with richer tool surfaces and `server.source = "sandbox"`.

// Scaffold-only: the API surface is intentionally not wired into the
// CLI dispatch yet. Tests reference these symbols; no production
// caller does. Clippy's dead-code lint isn't the signal we want here.
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use mcp_recon_core::McpServer;

/// Per-corpus-entry sandbox config. Today only the timeout is honoured;
/// the rest are placeholders for the real provider integration.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum wall-clock seconds per package (provisioning + install
    /// + handshake + teardown). Hard-stop to bound dollar cost.
    pub timeout_secs: u32,
    /// Provider identifier. Reserved for `"vercel"` / `"firecracker"` /
    /// `"docker"` etc. once a provider is wired.
    pub provider: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 120,
            provider: "vercel".to_string(),
        }
    }
}

/// Entry point matching [`super::npm::fetch_server`] / [`super::pypi::fetch_server`]
/// so the producer orchestrator can dispatch identically.
///
/// **Not yet implemented.** Returns an explicit error pointing at the
/// design doc. The orchestrator should call this when
/// `--with-sandbox` is set on the corpus walk; until provider wiring
/// lands, the corpus entry is logged-and-skipped exactly like any
/// other producer failure.
pub fn fetch_server(handle: &str, _config: &SandboxConfig) -> Result<McpServer> {
    Err(anyhow!(
        "sandbox producer not yet wired (see docs/SANDBOX-PRODUCER.md). \
         Corpus entry `{handle}` skipped.",
    ))
}

/// Orchestration shape — what the real implementation will look like.
/// Kept as a deliberately-empty function so the scaffold compiles and
/// any future caller wires in the same shape.
#[allow(dead_code)]
fn produce_sandboxed(_handle: &str, _config: &SandboxConfig) -> Result<McpServer> {
    // 1. Resolve handle → package name + version + ecosystem (npm/pypi)
    //    (use `super::corpus::ParsedHandle::from_handle`)
    //
    // 2. Provision a microVM via the provider API
    //    (Vercel Sandbox: POST /v1/sandboxes with the runtime hint)
    //
    // 3. Inside the VM:
    //      - npm install <name>@<version>   OR   pip install <name>==<version>
    //      - spawn the package's bin/entry_point as a stdio subprocess
    //      - write JSON-RPC frames:
    //          {"jsonrpc":"2.0","id":1,"method":"initialize",...}
    //          {"jsonrpc":"2.0","method":"notifications/initialized"}
    //          {"jsonrpc":"2.0","id":2,"method":"tools/list"}
    //      - read responses until tools/list returns
    //      - normalize the live tools/list payload into Vec<Tool>
    //      - send shutdown / kill subprocess
    //
    // 4. Tear down the VM (provider DELETE /v1/sandboxes/<id>)
    //
    // 5. Return McpServer { name, transport: Stdio, tools }
    //
    // Error budget: any step can fail. The classifier + leaderboard
    // continue with whatever finished — same one-bad-package-doesn't-
    // tank-corpus-walk contract as the registry producer.
    unreachable!("scaffold — produce_sandboxed not yet wired")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_returns_explicit_not_implemented_error() {
        let cfg = SandboxConfig::default();
        let err = fetch_server("npm:@scope/x@1.0.0", &cfg).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not yet wired"));
        assert!(msg.contains("docs/SANDBOX-PRODUCER.md"));
        assert!(msg.contains("npm:@scope/x@1.0.0"));
    }

    #[test]
    fn default_config_has_safe_timeout() {
        let cfg = SandboxConfig::default();
        assert!(cfg.timeout_secs > 0);
        assert!(cfg.timeout_secs <= 600);
        assert_eq!(cfg.provider, "vercel");
    }
}
