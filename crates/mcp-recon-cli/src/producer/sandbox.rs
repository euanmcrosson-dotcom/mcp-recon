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
use std::io::{Read, Write};
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

/// Spawn `cmd`, feed it `stdin_data`, and collect its output under a
/// wall-clock `deadline`.
///
/// stdout and stderr are drained on their own threads *while* the child
/// runs. That is not a nicety: a child writing more than the OS pipe
/// buffer (~64 KiB) blocks mid-write until someone reads, so waiting for
/// exit before reading deadlocks. The previous implementation polled
/// `try_wait()` and only called `wait_with_output()` after the child had
/// exited, which meant any server with a large `tools/list` could never
/// finish and always hit the budget — no matter how large the budget was.
fn spawn_drain_wait(
    mut cmd: Command,
    stdin_data: &[u8],
    deadline: Duration,
) -> Result<std::process::Output> {
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context("spawning child process")?;

    // Drain both pipes immediately, before anything can block.
    let mut out_pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("child: no stdout"))?;
    let mut err_pipe = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("child: no stderr"))?;
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });

    // Writing stdin can itself block if the child isn't reading yet, so
    // it happens after the readers are live. Dropping the handle closes
    // stdin, which the shim relies on to see EOF.
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("child: no stdin"))?;
        stdin
            .write_all(stdin_data)
            .context("writing script to child stdin")?;
    }

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None => {
                if start.elapsed() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Join so the reader threads don't outlive this call.
                    let _ = out_reader.join();
                    let _ = err_reader.join();
                    return Err(anyhow!("timed out after {}s", deadline.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    };

    let stdout = out_reader
        .join()
        .map_err(|_| anyhow!("stdout reader thread panicked"))?;
    let stderr = err_reader
        .join()
        .map_err(|_| anyhow!("stderr reader thread panicked"))?;

    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
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

    let output = spawn_drain_wait(
        cmd,
        script.as_bytes(),
        Duration::from_secs(u64::from(config.timeout_secs)),
    )
    .map_err(|e| {
        // Preserve the historical message so callers/logs are unchanged.
        if e.to_string().contains("timed out") {
            anyhow!("sandbox timed out after {}s", config.timeout_secs)
        } else {
            e
        }
    })?;

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

    /// A server whose `tools/list` is larger than the OS pipe buffer
    /// (~64 KiB) must still be captured.
    ///
    /// Regression test for a deadlock: `run_docker` used to write the
    /// script to stdin and then poll `try_wait()` without draining
    /// stdout, only calling `wait_with_output()` once the child had
    /// already exited. A child that writes more than the pipe buffer
    /// blocks mid-write, so it can never exit, so `try_wait()` never
    /// returns `Some` — the run always hit the wall-clock budget no
    /// matter how large the budget was. firecrawl-mcp@3.34.1 emits
    /// ~83 KiB and was silently dropped from every leaderboard run.
    #[test]
    fn captures_child_output_larger_than_the_pipe_buffer() {
        if std::process::Command::new("node")
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("skipping: node not on PATH");
            return;
        }
        const BYTES: usize = 200_000; // comfortably over any pipe buffer
        let mut cmd = Command::new("node");
        cmd.arg("-e")
            .arg(format!("process.stdout.write('x'.repeat({BYTES}))"));

        // A short deadline keeps the failure mode a clean timeout error
        // rather than a hung test run.
        let out = spawn_drain_wait(cmd, b"", Duration::from_secs(20))
            .expect("child writing 200 KiB should complete, not deadlock");

        assert!(out.status.success(), "child exited non-zero");
        assert_eq!(
            out.stdout.len(),
            BYTES,
            "stdout truncated: got {} of {BYTES} bytes",
            out.stdout.len()
        );
    }

    /// The deadline must still fire for a child that genuinely hangs.
    #[test]
    fn still_times_out_on_a_child_that_never_exits() {
        if std::process::Command::new("node")
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("skipping: node not on PATH");
            return;
        }
        let mut cmd = Command::new("node");
        cmd.arg("-e").arg("setInterval(() => {}, 1000)");

        let err = spawn_drain_wait(cmd, b"", Duration::from_secs(2))
            .expect_err("a child that never exits must hit the deadline");
        assert!(
            err.to_string().contains("timed out"),
            "unexpected error: {err}"
        );
    }

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
        let s = build_npm_script("@modelcontextprotocol/server-everything", "2026.1.26", &[]);
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
