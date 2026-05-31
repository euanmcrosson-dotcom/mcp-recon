// Capframe sandbox shim — runs inside the Docker container.
// Spawns the installed MCP server, performs the canonical
//   initialize → notifications/initialized → tools/list
// handshake, and writes the tools/list result to stdout wrapped in
// markers the parent process can find amid any server logging.

const { spawn } = require("child_process");
const readline = require("readline");
const fs = require("fs");
const path = require("path");

// Pure resolution: given a package name and its package.json metadata,
// decide how to launch its MCP server. Returns a descriptor; findBin()
// maps it to absolute /w/node_modules paths. Kept side-effect-free so it
// can be unit-tested (see sandbox_shim.test.js).
//
//   { type: "bin",  leaf, target }  -> launcher at .bin/<leaf>
//   { type: "node", target }        -> run `node <pkg>/<target>`
//   { type: "error", msg }
function resolveBin(pkg, meta) {
  const bin = meta.bin;
  if (typeof bin === "string") {
    // string bin → npm names the launcher after the package's unscoped name
    const leaf = pkg.startsWith("@") ? pkg.split("/").pop() : pkg;
    return { type: "bin", leaf, target: bin };
  }
  if (bin && typeof bin === "object" && Object.keys(bin).length > 0) {
    const name = Object.keys(bin)[0];
    // npm names the launcher after the BASENAME of the bin name — it
    // strips any directory component. A package can declare a bin name
    // containing a slash (e.g. e2b: {"@e2b/mcp-server": "..."}), which
    // npm installs as `.bin/mcp-server`, not `.bin/@e2b/mcp-server`.
    return { type: "bin", leaf: path.basename(name), target: bin[name] };
  }
  if (meta.main) {
    return { type: "node", target: meta.main };
  }
  return { type: "error", msg: "package has no bin or main" };
}

// Discover the bin to spawn. Reads the installed package's package.json
// and resolves it to an absolute command under /w/node_modules.
function findBin(pkg) {
  const pkgDir = path.join("/w", "node_modules", pkg);
  const pkgJsonPath = path.join(pkgDir, "package.json");
  if (!fs.existsSync(pkgJsonPath)) {
    process.stderr.write(`no package.json at ${pkgJsonPath}\n`);
    process.exit(3);
  }
  const meta = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
  const r = resolveBin(pkg, meta);
  if (r.type === "error") {
    process.stderr.write(r.msg + "\n");
    process.exit(4);
  }
  if (r.type === "node") {
    return ["node", [path.join(pkgDir, r.target)]];
  }
  // r.type === "bin": prefer the launcher npm created under .bin/; if
  // it isn't there (odd bin declarations, missing symlink), fall back to
  // running the bin's target entry directly with node.
  const binPath = "/w/node_modules/.bin/" + r.leaf;
  if (fs.existsSync(binPath)) {
    return [binPath, []];
  }
  if (r.target) {
    return ["node", [path.join(pkgDir, r.target)]];
  }
  process.stderr.write(`bin launcher not found: ${binPath}\n`);
  process.exit(4);
}

function main() {
  const pkgName = process.argv[2];
  if (!pkgName) {
    process.stderr.write("usage: shim.js <package-name> [server-arg ...]\n");
    process.exit(2);
  }
  // Any additional positional args are forwarded to the spawned MCP
  // server — used for servers that demand startup args (e.g.
  // server-postgres takes a database URL as argv[1]).
  const extraArgs = process.argv.slice(3);

  const [cmd, baseArgs] = findBin(pkgName);
  const args = baseArgs.concat(extraArgs);
  const proc = spawn(cmd, args, {
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, NODE_ENV: "production" },
  });

  // Buffer stderr but don't print it unless we fail.
  let stderrBuf = "";
  proc.stderr.on("data", (chunk) => {
    stderrBuf += chunk.toString();
    if (stderrBuf.length > 8192) stderrBuf = stderrBuf.slice(-8192);
  });

  const rl = readline.createInterface({ input: proc.stdout });
  let id = 0;
  const nextId = () => ++id;
  let initId = null;
  let toolsId = null;
  let captured = false;
  let timedOut = false;

  function send(obj) {
    proc.stdin.write(JSON.stringify(obj) + "\n");
  }

  initId = nextId();
  send({
    jsonrpc: "2.0",
    id: initId,
    method: "initialize",
    params: {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "capframe-mcp-recon-sandbox", version: "0.1.0" },
    },
  });

  rl.on("line", (line) => {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    if (msg.id === initId && msg.result) {
      send({ jsonrpc: "2.0", method: "notifications/initialized" });
      toolsId = nextId();
      send({ jsonrpc: "2.0", id: toolsId, method: "tools/list" });
      return;
    }
    if (msg.id === toolsId && msg.result) {
      captured = true;
      // Use a marker so the parent can find this amid any server logging.
      process.stdout.write(
        "___CAPFRAME_TOOLS_LIST_START___" +
          JSON.stringify(msg.result) +
          "___CAPFRAME_TOOLS_LIST_END___\n",
      );
      proc.kill();
      setTimeout(() => process.exit(0), 250);
    }
  });

  const timeout = setTimeout(() => {
    timedOut = true;
    proc.kill();
    process.stderr.write("CAPFRAME_TIMEOUT\n" + stderrBuf);
    process.exit(5);
  }, 25000);

  proc.on("error", (err) => {
    clearTimeout(timeout);
    process.stderr.write("CAPFRAME_SPAWN_ERROR: " + err.message + "\n");
    process.exit(6);
  });

  proc.on("exit", (code) => {
    clearTimeout(timeout);
    if (!captured && !timedOut) {
      process.stderr.write(
        "CAPFRAME_PROC_EXIT before tools/list: code=" + code + "\n" + stderrBuf,
      );
      process.exit(7);
    }
  });
}

if (require.main === module) {
  main();
}

module.exports = { resolveBin, findBin };
