// Unit test for the sandbox shim's bin resolution (pure part).
// Run: node crates/mcp-recon-cli/src/producer/sandbox_shim.test.js
//
// Regression guard for the scoped-bin-name bug: a package may declare a
// `bin` NAME containing a slash (e.g. e2b: {"@e2b/mcp-server": "..."}).
// npm installs the launcher at `.bin/<basename>` (it strips the dir
// part), so the shim must take the basename to find it.

const assert = require("node:assert");
const { resolveBin } = require("./sandbox_shim.js");

// 1. THE BUG: bin name has a slash → npm installs `.bin/mcp-server`,
//    NOT `.bin/@e2b/mcp-server`. Resolver must return the basename.
assert.strictEqual(
  resolveBin("@e2b/mcp-server", {
    bin: { "@e2b/mcp-server": "./build/index.js" },
  }).leaf,
  "mcp-server",
  "bin NAME with a slash must resolve to its basename",
);

// 2. Regression: ordinary object bin (no slash) is unchanged.
assert.strictEqual(
  resolveBin("@modelcontextprotocol/server-github", {
    bin: { "mcp-server-github": "dist/index.js" },
  }).leaf,
  "mcp-server-github",
  "no-slash object bin must be unchanged",
);

// 3. String bin → launcher is named after the unscoped package name.
assert.strictEqual(
  resolveBin("exa-mcp-server", { bin: "build/index.js" }).leaf,
  "exa-mcp-server",
);
assert.strictEqual(
  resolveBin("@scope/thing", { bin: "cli.js" }).leaf,
  "thing",
  "scoped package + string bin → unscoped package name",
);

// 4. No bin → fall back to running `main` with node.
const m = resolveBin("weird-pkg", { main: "index.js" });
assert.strictEqual(m.type, "node");
assert.strictEqual(m.target, "index.js");

console.log("ok - sandbox_shim resolveBin: all cases pass");
