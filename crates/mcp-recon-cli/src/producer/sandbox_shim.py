"""Capframe PyPI sandbox shim — runs inside the Docker container.

Mirror of sandbox_shim.js for npm. Discovers the installed package's
console_scripts entry point, spawns it as the MCP server, performs the
canonical initialize -> notifications/initialized -> tools/list
handshake, and writes the tools/list result to stdout wrapped in the
same marker pair the npm shim uses so the parent process parser is
agnostic to the source registry.

Exit codes match sandbox_shim.js where possible:
  2 - missing argv
  3 - no installed dist metadata for the requested package
  4 - package has no console_scripts entry point
  5 - handshake timed out
  6 - server process failed to spawn
  7 - server process exited before tools/list result captured
  8 - reserved (used by the host shell script for `pip install` failure)
"""

from __future__ import annotations

import importlib.metadata as ilmd
import json
import os
import shlex
import shutil
import signal
import subprocess
import sys
import threading
import time
from typing import IO, Optional, Tuple

MARK_START = "___CAPFRAME_TOOLS_LIST_START___"
MARK_END = "___CAPFRAME_TOOLS_LIST_END___"
HANDSHAKE_TIMEOUT_S = 25.0


def find_entry(pkg_name: str) -> Tuple[str, list[str]]:
    """Return (command, args) to spawn the MCP server.

    Strategy:
    1. Find the installed Distribution by package name (normalized).
    2. Iterate its `console_scripts` entry points. If exactly one, take
       it. If multiple, prefer the one whose name matches the package
       or contains 'mcp'.
    3. If no console_scripts, try `python -m <top_level_module>` where
       top_level_module is discovered from `top_level.txt` or RECORD.
    """
    try:
        dist = ilmd.distribution(pkg_name)
    except ilmd.PackageNotFoundError:
        # Try the normalized form (PyPI uses '-' but importable uses '_').
        try:
            dist = ilmd.distribution(pkg_name.replace("_", "-"))
        except ilmd.PackageNotFoundError:
            sys.stderr.write(f"CAPFRAME: no installed metadata for {pkg_name}\n")
            sys.exit(3)

    eps = [ep for ep in dist.entry_points if ep.group == "console_scripts"]
    if eps:
        # Prefer name == package, then anything with "mcp" in it, then first.
        norm = pkg_name.lower().replace("_", "-")
        def score(ep: ilmd.EntryPoint) -> int:
            n = ep.name.lower()
            if n == norm:
                return 0
            if "mcp" in n:
                return 1
            return 2
        eps.sort(key=score)
        chosen = eps[0]
        bin_path = shutil.which(chosen.name)
        if bin_path:
            return bin_path, []
        # Fall through if PATH doesn't have it — try python -m on the module.

    # No console_scripts (or its bin isn't on PATH). Try python -m on
    # the top-level module discovered from the dist.
    top = _top_level(dist)
    if top:
        return sys.executable, ["-m", top]

    sys.stderr.write(
        f"CAPFRAME: package {pkg_name} has no console_scripts and no "
        f"discoverable top-level module\n"
    )
    sys.exit(4)


def _top_level(dist: ilmd.Distribution) -> Optional[str]:
    # 1. Standard top_level.txt (setuptools-built dists).
    try:
        txt = dist.read_text("top_level.txt") or ""
        for line in txt.splitlines():
            line = line.strip()
            if line and not line.startswith("_"):
                return line
    except Exception:
        pass
    # 2. Fall back to the first directory-style entry in RECORD that
    #    looks like a Python package (has __init__.py).
    try:
        records = dist.files or []
        for f in records:
            parts = str(f).split("/")
            if len(parts) >= 2 and parts[-1] == "__init__.py":
                return parts[0]
    except Exception:
        pass
    return None


def main() -> None:
    if len(sys.argv) < 2:
        sys.stderr.write("usage: shim.py <package-name>\n")
        sys.exit(2)
    pkg = sys.argv[1]

    cmd, args = find_entry(pkg)

    env = dict(os.environ)
    env["PYTHONUNBUFFERED"] = "1"
    env.setdefault("PYTHONIOENCODING", "utf-8")

    try:
        proc = subprocess.Popen(
            [cmd, *args],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=False,
            bufsize=0,
        )
    except OSError as e:
        sys.stderr.write(f"CAPFRAME_SPAWN_ERROR: {e}\n")
        sys.exit(6)

    # Buffer stderr for failure diagnostics, capped.
    stderr_buf: list[bytes] = []

    def drain_stderr() -> None:
        assert proc.stderr is not None
        while True:
            chunk = proc.stderr.read(4096)
            if not chunk:
                return
            stderr_buf.append(chunk)
            # Cap total kept to ~16KB.
            total = sum(len(x) for x in stderr_buf)
            while total > 16384 and len(stderr_buf) > 1:
                total -= len(stderr_buf.pop(0))

    threading.Thread(target=drain_stderr, daemon=True).start()

    assert proc.stdin is not None and proc.stdout is not None

    def send(obj: dict) -> None:
        assert proc.stdin is not None
        line = (json.dumps(obj) + "\n").encode("utf-8")
        proc.stdin.write(line)
        proc.stdin.flush()

    next_id = 0

    def gen_id() -> int:
        nonlocal next_id
        next_id += 1
        return next_id

    init_id = gen_id()
    send(
        {
            "jsonrpc": "2.0",
            "id": init_id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "capframe-mcp-recon-sandbox",
                    "version": "0.1.0",
                },
            },
        }
    )

    tools_id: Optional[int] = None
    captured = False
    deadline = time.monotonic() + HANDSHAKE_TIMEOUT_S

    # Read newline-delimited JSON-RPC from server's stdout.
    buf = b""
    while not captured and time.monotonic() < deadline:
        try:
            chunk = proc.stdout.read(4096)
        except Exception:
            break
        if not chunk:
            # EOF before we got tools/list.
            break
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            if not line.strip():
                continue
            try:
                msg = json.loads(line.decode("utf-8"))
            except Exception:
                continue
            mid = msg.get("id")
            if mid == init_id and "result" in msg:
                send({"jsonrpc": "2.0", "method": "notifications/initialized"})
                tools_id = gen_id()
                send({"jsonrpc": "2.0", "id": tools_id, "method": "tools/list"})
                continue
            if tools_id is not None and mid == tools_id and "result" in msg:
                sys.stdout.write(
                    MARK_START + json.dumps(msg["result"]) + MARK_END + "\n"
                )
                sys.stdout.flush()
                captured = True
                break

    # Teardown.
    try:
        proc.send_signal(signal.SIGTERM)
    except Exception:
        pass
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        try:
            proc.kill()
        except Exception:
            pass

    if not captured:
        # If we got past the spawn but never saw tools/list, surface stderr.
        sys.stderr.write(
            b"CAPFRAME_NO_RESULT: handshake did not return tools/list\n"
            + b"".join(stderr_buf)
        )
        # Distinguish timeout from early exit for the host log.
        sys.exit(5 if proc.returncode is None else 7)


if __name__ == "__main__":
    main()
