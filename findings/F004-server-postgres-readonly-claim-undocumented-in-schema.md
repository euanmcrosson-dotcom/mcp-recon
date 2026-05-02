# F004 — `query` tool description claims "read-only" but schema doesn't enforce

| Field | Value |
|---|---|
| **Server** | `@modelcontextprotocol/server-postgres` v0.1.0 (npm `0.6.2`) |
| **Tool** | `query` |
| **Discovery date** | 2026-05-01 |
| **Severity** | low |
| **Severity rationale** | Capability-surface observation. The upstream description says "Run a read-only SQL query"; the implementation enforces this via a `BEGIN; ROLLBACK;` wrapper around the user-supplied SQL, which is correct *but is not visible to the recon scan or to capnagent*. From the outside, the schema is `{type: "object", properties: {sql: {type: "string"}}}` — completely unbound. An operator running this server pointed at a real database needs to bound `arg.sql` *externally* if the agent shouldn't have full read of every accessible table. The classifier flags this correctly as `write/confused-deputy` because the description's "read-only" promise is not auditable from the schema. |
| **Coordinated disclosure status** | not-applicable-published-as-deprecated-example |

## Description

The mcp-recon enumeration of `@modelcontextprotocol/server-postgres`
reveals exactly one tool:

```json
{
  "name": "query",
  "description": "Run a read-only SQL query",
  "inputSchema": {
    "type": "object",
    "properties": { "sql": { "type": "string" } }
  }
}
```

The classifier's rationale:

```
description has side-effect verb → escalate one step
user-controllable string arg + non-read authority → confused-deputy candidate
```

Recommended caveat:

```
tool == "query" AND caller == "<your-caller-id>" AND now <= @<your-cap-expiry>
  // WRITE unclassified — operator must review
```

Inspection of the upstream source
(`@modelcontextprotocol/server-postgres/dist/index.js`) confirms
the implementation wraps every query in `BEGIN TRANSACTION READ
ONLY; ... ROLLBACK;` — which *does* enforce the read-only invariant
at the Postgres level. That's a real defence. But it's invisible
to:

- The recon scan, which only sees the JSON-Schema
- capnagent, which can only constrain the shape of the call
- A future fork of the server that might silently drop the
  transaction wrapper

Crucially, "read-only" still leaves the entire **read** surface
open. A SQL injection inside an injected tool call (e.g. a
prompt-injected `SELECT secret FROM api_keys`) is not blocked by
the read-only transaction — the read-only invariant only stops
data modification, not exfiltration.

## Reproduction

```bash
mcp-recon scan \
  "stdio:npx -y @modelcontextprotocol/server-postgres postgresql://invalid:invalid@localhost:5432/invalid" \
  --out=examples/public-servers/server-postgres --budget=200
```

(The connection string is intentionally bogus — the server
enumerates its tool surface before opening the database
connection, so the inventory is recoverable without a live DB.)

Expected: `mcp-recon: 1 tools, 1 confused-deputy candidates`. The
classifier's output is in
`examples/public-servers/server-postgres/classification.json`.

## Recommended capnagent caveat

The base caveat from the classifier is the floor; for this
finding, an operator who **does** rely on the upstream's
read-only invariant should still bound the query shape:

```
tool == "query"
  AND caller == "agent:reporting"
  AND arg.sql starts_with "SELECT "
  AND arg.sql NOT contains "pg_read_server_files"
  AND arg.sql NOT contains "COPY "
  AND arg.sql NOT contains "lo_export"
  AND now <= @2026-12-31T23:59:59Z
```

The `starts_with "SELECT "` predicate is a very rough belt: it
doesn't catch `WITH ... SELECT`, function-call SQL, or `EXECUTE`.
An operator who needs hard guarantees should run the agent
against a Postgres role with column-level GRANTs configured
upstream of the MCP server, not lean on capnagent string
matching.

## Why this is "low" not "informational"

This is "low" rather than "informational" because the gap between
the description's promise and the schema's enforcement is exactly
the kind of confused-deputy gap the recon tool exists to surface.
An operator reading the description in isolation might
reasonably conclude "the server enforces read-only, I don't need
to caveat further." That's a load-bearing wrong assumption for an
agent with prompt-injection exposure. The finding's value is
exactly to push back on that assumption.

## Coordinated disclosure

Not applicable. The upstream package is npm-deprecated:

```
npm warn deprecated @modelcontextprotocol/server-postgres@0.6.2:
  Package no longer supported.
```

Anthropic has not replaced it in the active reference set; the
expected operator path is "use a different Postgres MCP server"
or "write your own with explicit caveat-friendly tool shapes".
