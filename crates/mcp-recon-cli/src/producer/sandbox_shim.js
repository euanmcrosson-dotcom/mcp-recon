// Capframe sandbox shim — runs inside the Docker container.
// Spawns the installed MCP server, performs the canonical
//   initialize → notifications/initialized → tools/list
// handshake, and writes the tools/list result to stdout wrapped in
// markers the parent process can find amid any server logging.

const { spawn } = require("child_process");
const readline = require("readline");
const fs = require("fs");
const path = require("path");

const pkgName = process.argv[2];
if (!pkgName) {
  process.stderr.write("usage: shim.js <package-name> [server-arg ...]\n");
  process.exit(2);
}
// Any additional positional args are forwarded to the spawned MCP
// server — used for servers that demand startup args (e.g.
// server-postgres takes a database URL as argv[1]).
const extraArgs = process.argv.slice(3);

// Discover the bin to spawn. npm packages declare it under
// `bin` in package.json — either a single string (named after the
// package) or an object { name: path }.
function findBin(pkg) {
  const pkgJsonPath = path.join("/w", "node_modules", pkg, "package.json");
  if (!fs.existsSync(pkgJsonPath)) {
    process.stderr.write(`no package.json at ${pkgJsonPath}\n`);
    process.exit(3);
  }
  const meta = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
  const bin = meta.bin;
  if (typeof bin === "string") {
    const leaf = pkg.startsWith("@") ? pkg.split("/").pop() : pkg;
    return ["/w/node_modules/.bin/" + leaf, []];
  }
  if (bin && typeof bin === "object") {
    const name = Object.keys(bin)[0];
    return ["/w/node_modules/.bin/" + name, []];
  }
  // Fallback: try the `main` entry directly.
  if (meta.main) {
    return ["node", [path.join("/w/node_modules", pkg, meta.main)]];
  }
  process.stderr.write("package has no bin or main\n");
  process.exit(4);
}

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
