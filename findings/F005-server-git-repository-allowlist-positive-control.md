# F005 — `mcp-server-git --repository` is a positive defence-in-depth control

| Field | Value |
|---|---|
| **Server** | `mcp-server-git` v1.27.0 (PyPI `mcp-server-git==2026.1.14`) |
| **Tools** | All 12 (`git_status`, `git_diff_*`, `git_log`, `git_show`, `git_init`, `git_checkout`, `git_create_branch`, `git_add`, `git_commit`, `git_reset`, `git_branch`) |
| **Discovery date** | 2026-05-01 |
| **Severity** | informational (positive observation) |
| **Severity rationale** | Not a finding *against* the server — a finding *for* it. `mcp-server-git` is the only server in the dataset that enforces a server-side allowlist on the resource it operates on, configured at server start time via the `--repository <path>` CLI flag. Every fuzz call that supplies a `repo_path` outside that boundary is rejected with `Repository path 'X' is outside the allowed repository '<sandbox>'`. Other servers in the dataset accept any path the agent supplies and lean on capnagent (or nothing) to bound it. This pattern should be the floor for new MCP servers; the finding documents it so future server reviewers can cite it. |
| **Coordinated disclosure status** | n/a (positive observation) |

## Description

The mcp-recon scan of `mcp-server-git` (with sandbox repo at
`C:/tmp/mcp-recon-sandbox-git`) produces 792 fuzz calls, of which
**every single one** lands as `protocol_error`. The breakdown of
the error messages tells the story:

| Error pattern | Count |
|---|---|
| `Repository path 'X' is outside the allowed repository 'C:\tmp\mcp-recon-sandbox-git'` | 210 |
| `Input validation error: 'repo_path' is a required property` | 60 |
| `Input validation error: None is not of type 'string'` | 37 |
| `Input validation error: 0 is not of type 'array'` | 23 |
| `Input validation error: 3.14 is not of type 'string'` | 18 |
| (other Pydantic strict-validation errors) | ~444 |

The first row is the load-bearing one: 210 of 792 calls
specifically attempted to escape the configured repository (with
inputs like `repo_path: "/etc"`, `repo_path: "../../"`,
`repo_path: "C:\\Windows"`, `repo_path: "x"`) and the server
rejected every one with a uniform error message. The check is in
`mcp_server_git/server.py`:

```python
def _resolve_repo_path(self, requested: str) -> Path:
    p = Path(requested).resolve()
    allowed = self._allowed_repository.resolve()
    if not p.is_relative_to(allowed):
        raise ValueError(
            f"Repository path '{requested}' is outside the allowed "
            f"repository '{self._allowed_repository}'"
        )
    return p
```

Compare this to `@modelcontextprotocol/server-filesystem`, which
has a similar `--allowed-directories` argument but enforces it
*per-tool* in the implementation rather than at a single
`_resolve_repo_path` chokepoint, and accepts symlinks-out-of-tree
unless the operator explicitly disables symlink resolution.
`mcp-server-git`'s pattern is tighter.

## Reproduction

```bash
# Sandbox repo
mkdir -p /c/tmp/mcp-recon-sandbox-git
cd /c/tmp/mcp-recon-sandbox-git && git init .

# pip install mcp-server-git
mcp-recon scan \
  "stdio:python -m mcp_server_git --repository C:/tmp/mcp-recon-sandbox-git" \
  --out=examples/public-servers/server-git --budget=200
```

Expected: `mcp-recon: 12 tools, 0 confused-deputy candidates`,
followed by `fuzz — ok=0 protocol_error=792 runtime_error=0`.

To verify the allowlist specifically:

```bash
node -e '
const fz=require("./examples/public-servers/server-git/fuzz.json");
const allowlist=fz.calls.filter(c=>{
  const m=c.outcome.message||c.outcome.snippet||"";
  return m.includes("outside the allowed repository");
});
console.log("allowlist rejections:", allowlist.length);
'
```

## Recommendation for future MCP server authors

This pattern is what new MCP servers exposing scoped resources
(filesystem, git, database, cloud-storage) should adopt:

1. **Take the scope as a CLI flag at server start.** Don't accept
   it as a tool argument — the agent shouldn't be able to escape
   by supplying a different scope per call.
2. **Resolve and pin the scope once.** Keep the resolved
   absolute path in server state.
3. **Centralise the boundary check.** A single
   `_resolve_X(requested)` helper, called by every tool that
   takes a path/key/ID. Don't sprinkle the check into individual
   tool handlers.
4. **Reject with a uniform error.** Both for the operator's
   monitoring (one log pattern to grep for) and for the agent's
   robustness (a single error class to handle).

When the `mcp-recon caveats` command lands (currently in
[`feat/caveats-command`](https://github.com/euanmcrosson-dotcom/mcp-recon/tree/feat/caveats-command)),
it should learn to *recognise* this pattern: a server with a CLI
flag named `--repository` / `--allowed-directories` /
`--scope` should produce caveats that bind to that scope rather
than re-asserting a `arg.repo_path starts_with` predicate the
server already enforces.

## Why this matters for capnagent

capnagent's `mcp-fs-agent` integration assumes the underlying
MCP server is *not* enforcing its own boundary, so every caveat
re-asserts `arg.path starts_with /var/agent-sandbox/...`. For
`mcp-server-git`, that's redundant — the server already enforces
it. The capnagent issuer can be tightened to detect the
`--repository` flag at agent-config time and skip the
`arg.repo_path starts_with` predicate, reducing capnagent's
caveat surface.

(The right way to express that in the issuance plan format is
the `per_tool_overrides` mechanism documented in
`packages/mcp-recon-cli/src/caveats/`. That's a v0.2 enhancement.)
