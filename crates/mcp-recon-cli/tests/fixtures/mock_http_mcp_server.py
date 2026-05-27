#!/usr/bin/env python3
"""Minimal mock MCP server over Streamable HTTP, for integration tests.

Binds 127.0.0.1:0 (OS-assigned port), prints `PORT <n>` to stdout, then serves
JSON-RPC over POST: `initialize` (with an Mcp-Session-Id header), the
`notifications/initialized` notification (202, no body), and `tools/list`
(returns two tools). Responds as application/json.
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOOLS = [
    {
        "name": "http_exec",
        "description": "Execute a shell command on the remote host.",
        "inputSchema": {
            "type": "object",
            "properties": {"cmd": {"type": "string"}},
            "required": ["cmd"],
        },
    },
    {
        "name": "http_lookup",
        "description": "Look up a record by id.",
        "inputSchema": {
            "type": "object",
            "properties": {"id": {"type": "string", "maxLength": 64}},
        },
    },
]


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length) if length else b""
        req = json.loads(raw) if raw else {}
        method = req.get("method")
        rid = req.get("id")

        if method == "initialize":
            self._json(
                {
                    "jsonrpc": "2.0",
                    "id": rid,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "serverInfo": {"name": "mock-http", "version": "0.0.0"},
                    },
                },
                session=True,
            )
        elif method == "notifications/initialized":
            self.send_response(202)
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif method == "tools/list":
            self._json({"jsonrpc": "2.0", "id": rid, "result": {"tools": TOOLS}})
        else:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()

    def _json(self, obj, session=False):
        data = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        if session:
            self.send_header("Mcp-Session-Id", "test-session-1")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):  # silence access logging
        pass


def main():
    srv = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    print(f"PORT {srv.server_address[1]}", flush=True)
    sys.stdout.flush()
    srv.serve_forever()


if __name__ == "__main__":
    main()
