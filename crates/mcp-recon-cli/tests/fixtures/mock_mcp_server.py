#!/usr/bin/env python3
"""Minimal mock MCP server (stdio, newline-delimited JSON-RPC 2.0).

Used by the mcp-recon enumerate integration test. Implements just enough of
the protocol to exercise the client: responds to `initialize` and
`tools/list`, ignores the `notifications/initialized` notification. Exposes
two tools so the resulting inventory drives both R1 (unconstrained string)
and R7 (code-execution) in the classifier.
"""
import sys
import json


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            continue

        method = msg.get("method")
        mid = msg.get("id")

        if method == "initialize":
            send({
                "jsonrpc": "2.0",
                "id": mid,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "mock-mcp-server", "version": "0.0.0"},
                },
            })
        elif method == "notifications/initialized":
            # notification — no response
            pass
        elif method == "tools/list":
            send({
                "jsonrpc": "2.0",
                "id": mid,
                "result": {
                    "tools": [
                        {
                            "name": "execute_shell_command",
                            "description": "Execute a shell command for system management.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {"command": {"type": "string"}},
                                "required": ["command"],
                            },
                        },
                        {
                            "name": "read_file",
                            "description": "Read a file from disk.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {"path": {"type": "string"}},
                                "required": ["path"],
                            },
                        },
                    ]
                },
            })
        elif mid is not None:
            # Unknown request — return a JSON-RPC error so the client doesn't hang.
            send({
                "jsonrpc": "2.0",
                "id": mid,
                "error": {"code": -32601, "message": "method not found"},
            })


if __name__ == "__main__":
    main()
