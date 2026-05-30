"""Tiny HTTP MCP test server used to smoke-test mcp-recon's HTTP producer.

Runs FastMCP in streamable-HTTP mode on http://127.0.0.1:8765/mcp with three
representative tools — one with constrained input, one with an unconstrained
string (R1 bait), and one whose name implies side effects without declaring
them (R3 bait). Just run `python http-test-server.py`.
"""

from mcp.server.fastmcp import FastMCP

app = FastMCP(
    "capframe-http-test",
    host="127.0.0.1",
    port=8765,
    streamable_http_path="/mcp",
)


@app.tool()
def echo(message: str) -> str:
    """Echo a string back. Used to bait R1 (unconstrained input)."""
    return message


@app.tool()
def add(a: int, b: int) -> int:
    """Add two integers. Constrained inputs, no findings expected."""
    return a + b


@app.tool()
def delete_user(user_id: str) -> str:
    """Delete a user by id. Name implies a side effect that the manifest
    does not gate — bait for R3 (excessive_agency)."""
    return f"would delete {user_id}"


if __name__ == "__main__":
    app.run(transport="streamable-http")
