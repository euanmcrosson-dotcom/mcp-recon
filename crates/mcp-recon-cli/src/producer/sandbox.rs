//! Sandbox producer — high-fidelity path of the Capframe leaderboard
//! pipeline.
//!
//! Each corpus entry runs in an ephemeral Docker container: `npm install`
//! the package, spawn the MCP server over stdio, perform the canonical
//! `initialize` + `tools/list` handshake, capture the live tool surface,
//! tear down. Parameter schemas survive, so the classifier's R1/R2/R4
//! rules can fire — not just the name/description rules.
//!
//! Provider for now: local Docker. The original design doc
//! (`docs/SANDBOX-PRODUCER.md`) covered Vercel Sandbox / Firecracker as
//! the production target; Docker is the local equivalent that works on
//! the same `docker run` API surface and runs anywhere the producer
//! runs. Substituting a remote provider is a `Command::new` swap.
//!
//! Covers npm and PyPI. Each registry has its own shim that runs
//! inside the container (Node for npm, Python for PyPI). The shims
//! emit `tools/list` results wrapped in the same marker pair, so the
//! host parser is registry-agnostic.

use anyhow::{anyhow, Context, Result};
use mcp_recon_core::{McpServer, SideEffect, Tool, Transport};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Per-corpus-entry config. Hard caps wall-clock + memory so a single
/// hung server can't tank the corpus walk.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Hard timeout for the entire `docker run` invocation.
    pub timeout_secs: u32,
    /// Memory limit passed to `docker run --memory`.
    pub memory_mb: u32,
    /// Docker image used for npm entries. Defaults to `node:20`.
    pub image: String,
    /// Docker image used for PyPI entries. Defaults to `python:3.12-slim`.
    pub image_pypi: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 120,
            memory_mb: 512,
            image: "node:20".to_string(),
            image_pypi: "python:3.12-slim".to_string(),
        }
    }
}

/// Shim scripts — run inside the container and perform the MCP
/// handshake. Output is wrapped in the same markers regardless of
/// registry so the host parser is shared.
const SHIM_JS: &str = include_str!("./sandbox_shim.js");
const SHIM_PY: &str = include_str!("./sandbox_shim.py");

/// Entry point matching [`super::npm::fetch_server`] /
/// [`super::pypi::fetch_server`] so the corpus walker can dispatch
/// to the sandbox path without knowing about Docker.
pub fn fetch_server_npm(
    name: &str,
    version: &str,
    overrides: Option<&super::corpus::SandboxOverrides>,
    config: &SandboxConfig,
) -> Result<McpServer> {
    let (extra_args, env) = split_overrides(overrides);
    let script = build_npm_script(name, version, extra_args);
    let raw = run_docker(&script, &config.image, env, config)
        .with_context(|| format!("sandbox docker run for npm:{name}@{version}"))?;
    finalize(name, &raw)
}

/// Entry point for PyPI handles — mirrors `fetch_server_npm` shape.
/// Uses `pip install` + a Python shim instead of `npm install` + node.
pub fn fetch_server_pypi(
    name: &str,
    version: &str,
    overrides: Option<&super::corpus::SandboxOverrides>,
    config: &SandboxConfig,
) -> Result<McpServer> {
    let (extra_args, env) = split_overrides(overrides);
    let script = build_pypi_script(name, version, extra_args);
    let raw = run_docker(&script, &config.image_pypi, env, config)
        .with_context(|| format!("sandbox docker run for pypi:{name}@{version}"))?;
    finalize(name, &raw)
}

fn split_overrides(
    overrides: Option<&super::corpus::SandboxOverrides>,
) -> (
    &[String],
    Option<&std::collections::BTreeMap<String, String>>,
) {
    match overrides {
        Some(o) => (o.args.as_slice(), Some(&o.env)),
        None => (&[], None),
    }
}

fn finalize(name: &str, raw: &str) -> Result<McpServer> {
    let tools_list = extract_tools_list(raw)
        .ok_or_else(|| anyhow!("sandbox did not emit a tools/list result for {name}"))?;
    let inventory = normalise_tools_list(tools_list)?;
    Ok(McpServer {
        name: name.to_string(),
        transport: Some(Transport::Stdio),
        tools: inventory,
    })
}

fn build_npm_script(name: &str, version: &str, extra_args: &[String]) -> String {
    // Sequencing matters: we suppress install chatter on stdout (npm is
    // noisy) so the shim's marker line is the only thing on stdout when
    // the handshake succeeds.
    let args = format_extra_args(extra_args);
    format!(
        r#"set -e
mkdir -p /w
cd /w
npm init -y > /dev/null 2>&1
npm install {name}@{version} > /tmp/install.log 2>&1 || (cat /tmp/install.log >&2; exit 8)
cat > /w/shim.js <<'EOSHIM_CAPFRAME'
{SHIM_JS}
EOSHIM_CAPFRAME
node /w/shim.js {name}{args}
"#
    )
}

fn build_pypi_script(name: &str, version: &str, extra_args: &[String]) -> String {
    // python:3.12-slim has pip but is otherwise minimal. We install
    // into /usr (system site-packages) for simplicity — this is a
    // throwaway container.
    let args = format_extra_args(extra_args);
    format!(
        r#"set -e
mkdir -p /w
cd /w
pip install --quiet --root-user-action=ignore "{name}=={version}" >/tmp/install.log 2>&1 || (cat /tmp/install.log >&2; exit 8)
cat > /w/shim.py <<'EOSHIM_CAPFRAME'
{SHIM_PY}
EOSHIM_CAPFRAME
python /w/shim.py {name}{args}
"#
    )
}

/// POSIX-sh single-quote each extra argv element. The leading space
/// is included so the result can be concatenated directly after the
/// shim invocation.
fn format_extra_args(extra: &[String]) -> String {
    if extra.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for a in extra {
        out.push(' ');
        out.push('\'');
        for ch in a.chars() {
            if ch == '\'' {
                out.push_str("'\\''");
            } else {
                out.push(ch);
            }
        }
        out.push('\'');
    }
    out
}

/// Run a prepared shell script inside an ephemeral container. Returns
/// the container's combined stdout for the marker-parser.
fn run_docker(
    script: &str,
    image: &str,
    env: Option<&std::collections::BTreeMap<String, String>>,
    config: &SandboxConfig,
) -> Result<String> {
    let mut cmd = Command::new("docker");
    cmd.args([
        "run",
        "--rm",
        "-i",
        "--network",
        "bridge",
        "--memory",
        &format!("{}m", config.memory_mb),
        // Stop containers that ignore SIGTERM after the wall-clock
        // budget — defense against runaway servers.
        "--stop-timeout",
        "5",
    ]);
    // Forward per-entry env vars (`docker run -e KEY=VALUE`). The
    // corpus file is public, so values here are dummies that just let
    // the server boot far enough to advertise tools/list. Real
    // credentials live in GHA secrets and would be merged at the cron
    // layer if ever wanted.
    if let Some(m) = env {
        for (k, v) in m {
            cmd.arg("-e");
            cmd.arg(format!("{k}={v}"));
        }
    }
    cmd.arg(image);
    cmd.arg("sh");
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context("spawning docker run")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("docker run: no stdin"))?
        .write_all(script.as_bytes())
        .context("writing sandbox script to docker stdin")?;

    let start = std::time::Instant::now();
    let deadline = Duration::from_secs(u64::from(config.timeout_secs));
    let output = loop {
        match child.try_wait()? {
            Some(_) => break child.wait_with_output()?,
            None => {
                if start.elapsed() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(anyhow!(
                        "sandbox timed out after {}s",
                        config.timeout_secs
                    ));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "docker run exited {}: {}",
            output.status,
            stderr.lines().last().unwrap_or("(no stderr)")
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Pull the JSON body of the tools/list result out of the shim's
/// stdout. The shim emits a marker pair around the JSON so any server
/// logging is safely ignored.
fn extract_tools_list(stdout: &str) -> Option<&str> {
    const START: &str = "___CAPFRAME_TOOLS_LIST_START___";
    const END: &str = "___CAPFRAME_TOOLS_LIST_END___";
    let i = stdout.find(START)?;
    let body_start = i + START.len();
    let rel_end = stdout[body_start..].find(END)?;
    Some(&stdout[body_start..body_start + rel_end])
}

/// Map an MCP `tools/list` response into our `mcp_recon_core::Tool`
/// vec. The MCP shape is:
/// ```json
/// { "tools": [{ "name", "description", "inputSchema": {...} }, ...] }
/// ```
fn normalise_tools_list(raw_json: &str) -> Result<Vec<Tool>> {
    let v: Value = serde_json::from_str(raw_json).context("parse tools/list JSON")?;
    let tools = v
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("tools/list response missing `tools` array"))?;
    let mut out = Vec::with_capacity(tools.len());
    for t in tools {
        let name = t
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("tool entry missing `name`"))?
            .to_string();
        let description = t
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string);
        let parameters = t.get("inputSchema").cloned();
        // MCP doesn't carry side_effects/auth_required in tools/list yet,
        // but we infer "execute" from common name patterns so R7 has a
        // signal beyond what the static-manifest path already gives.
        let side_effects = infer_side_effects(&name);
        out.push(Tool {
            name,
            description,
            parameters,
            side_effects,
            auth_required: None,
            rate_limited: None,
        });
    }
    Ok(out)
}

fn infer_side_effects(name: &str) -> Vec<SideEffect> {
    // Light heuristic — the classifier still owns the R3 / R5 / R7 logic.
    // We add Execute only when the name strongly implies arbitrary
    // command execution, so R3 can fire on the gap between declared
    // and implied effects for OTHER mutation names.
    let n = name.to_lowercase();
    if n.contains("exec_") || n.starts_with("exec ") || n == "exec" || n.contains("run_command") {
        vec![SideEffect::Execute]
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_tools_list_between_markers() {
        let stdout = "noise before\n\
                      some logs\n\
                      ___CAPFRAME_TOOLS_LIST_START___{\"tools\":[{\"name\":\"x\"}]}___CAPFRAME_TOOLS_LIST_END___\n\
                      trailing noise\n";
        let extracted = extract_tools_list(stdout).unwrap();
        assert_eq!(extracted, r#"{"tools":[{"name":"x"}]}"#);
    }

    #[test]
    fn extract_returns_none_when_marker_missing() {
        assert!(extract_tools_list("plain server log line").is_none());
    }

    #[test]
    fn normalises_tools_list_with_input_schemas() {
        let raw = json!({
            "tools": [
                {
                    "name": "fetch_url",
                    "description": "Fetch a URL and return body",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "url": { "type": "string", "format": "uri" } },
                        "required": ["url"]
                    }
                },
                {
                    "name": "exec_shell",
                    "description": "Execute a shell command",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "cmd": { "type": "string" } }
                    }
                }
            ]
        });
        let tools = normalise_tools_list(&raw.to_string()).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "fetch_url");
        assert_eq!(
            tools[0].description.as_deref(),
            Some("Fetch a URL and return body")
        );
        assert!(tools[0].parameters.is_some());
        assert_eq!(tools[1].name, "exec_shell");
        assert!(tools[1]
            .side_effects
            .iter()
            .any(|s| matches!(s, SideEffect::Execute)));
    }

    #[test]
    fn errors_when_response_lacks_tools_array() {
        let err = normalise_tools_list(r#"{"foo":"bar"}"#).unwrap_err();
        assert!(err.to_string().contains("missing `tools` array"));
    }

    #[test]
    fn infers_execute_only_on_obvious_names() {
        assert!(matches!(
            infer_side_effects("exec_shell")[..],
            [SideEffect::Execute]
        ));
        assert!(matches!(
            infer_side_effects("run_command")[..],
            [SideEffect::Execute]
        ));
        assert!(infer_side_effects("read_file").is_empty());
        assert!(infer_side_effects("create_user").is_empty());
    }

    #[test]
    fn default_config_caps_compute() {
        let cfg = SandboxConfig::default();
        assert!(cfg.timeout_secs > 0);
        assert!(cfg.timeout_secs <= 600);
        assert!(cfg.memory_mb >= 256);
        assert_eq!(cfg.image, "node:20");
        assert_eq!(cfg.image_pypi, "python:3.12-slim");
    }

    #[test]
    fn pypi_script_uses_pip_install_with_pin() {
        let s = build_pypi_script("mcp-server-git", "1.2.3", &[]);
        assert!(s.contains("pip install"));
        assert!(s.contains("\"mcp-server-git==1.2.3\""));
        assert!(s.contains("python /w/shim.py mcp-server-git"));
        // Marker heredoc must terminate cleanly.
        assert!(s.contains("EOSHIM_CAPFRAME"));
    }

    #[test]
    fn npm_script_uses_npm_install_with_pin() {
        let s = build_npm_script(
            "@modelcontextprotocol/server-everything",
            "2026.1.26",
            &[],
        );
        assert!(s.contains("npm install @modelcontextprotocol/server-everything@2026.1.26"));
        assert!(s.contains("node /w/shim.js @modelcontextprotocol/server-everything"));
    }

    #[test]
    fn extra_args_are_single_quoted_in_npm_script() {
        let s = build_npm_script(
            "@modelcontextprotocol/server-postgres",
            "0.6.2",
            &["postgres://x:y@127.0.0.1:5432/z".to_string()],
        );
        assert!(s.contains(
            "node /w/shim.js @modelcontextprotocol/server-postgres 'postgres://x:y@127.0.0.1:5432/z'"
        ));
    }

    #[test]
    fn format_extra_args_escapes_embedded_single_quotes() {
        let out = format_extra_args(&["it's".to_string()]);
        assert_eq!(out, " 'it'\\''s'");
    }

    #[test]
    fn format_extra_args_is_empty_when_no_args() {
        assert_eq!(format_extra_args(&[]), "");
    }
}
