# mcp-recon performance benchmarks

These benchmarks measure the pure-function pipeline (`classify`,
`classify` + fuzz fold, `renderMarkdown`, `planCaveats`) plus a
synthetic-scale sweep at 10 / 100 / 1000 / 10 000 tools to confirm
the O(tools) claim in `docs/SPEC.md`.

The harness is `bench/run.ts`, driven by [tinybench]. Run with:

```sh
npm run bench --workspace=@mcp-recon/cli
```

Each task gets at least 100 iterations and 10 warm-ups (synthetic-scale
uses 30 + 3 with a 500 ms time budget per task, so the n=10000 case
gets enough samples for a stable mean). Per-task we report mean,
median, p95 (in milliseconds) and ops/sec.

[tinybench]: https://github.com/tinylibs/tinybench

## Baseline (commit dc98e83, Node v24.14.1, win32/x64)

Total wall-clock for the full suite: **~15 s**.

### Reference servers (4 published)

| Group            | Task                                       | Mean (ms) | Median (ms) | p95 (ms) | Ops/sec  |
| ---------------- | ------------------------------------------ | --------: | ----------: | -------: | -------: |
| classify         | classify(server-filesystem) (14 tools)     |    0.2458 |      0.2247 |   0.3481 |    4 068 |
| classify         | classify(server-everything) (8 tools)      |    0.0785 |      0.0667 |   0.1372 |   12 735 |
| classify         | classify(server-memory) (9 tools)          |    0.0448 |      0.0387 |   0.0739 |   22 313 |
| classify         | classify(server-sequential-thinking)       |    0.0594 |      0.0554 |   0.0790 |   16 840 |
| classify + fuzz  | classify+fuzz(server-filesystem)           |    0.2734 |      0.2073 |   0.5107 |    3 658 |
| classify + fuzz  | classify+fuzz(server-everything)           |    0.1594 |      0.1493 |   0.2561 |    6 273 |
| classify + fuzz  | classify+fuzz(server-memory)               |    0.0942 |      0.0982 |   0.1429 |   10 613 |
| classify + fuzz  | classify+fuzz(server-sequential-thinking)  |    0.1118 |      0.1052 |   0.1591 |    8 942 |
| renderMarkdown   | renderMarkdown(server-filesystem)          |    0.1274 |      0.1133 |   0.1960 |    7 848 |
| renderMarkdown   | renderMarkdown(server-everything)          |    0.0789 |      0.0744 |   0.1180 |   12 668 |
| renderMarkdown   | renderMarkdown(server-memory)              |    0.0469 |      0.0448 |   0.0677 |   21 335 |
| renderMarkdown   | renderMarkdown(server-sequential-thinking) |    0.0160 |      0.0161 |   0.0219 |   62 336 |
| planCaveats      | planCaveats(server-filesystem)             |    0.0913 |      0.0814 |   0.1349 |   10 957 |
| planCaveats      | planCaveats(server-everything)             |    0.0810 |      0.0839 |   0.1159 |   12 351 |
| planCaveats      | planCaveats(server-memory)                 |    0.0569 |      0.0591 |   0.0817 |   17 584 |
| planCaveats      | planCaveats(server-sequential-thinking)    |    0.0129 |      0.0129 |   0.0202 |   77 808 |

Every published server completes the full pure-function pipeline
(classify + fuzz fold + planCaveats + renderMarkdown) in **well under
one millisecond** end-to-end on the baseline machine.

### Synthetic scale — confirms O(tools)

| Tools  | classify mean (ms) | planCaveats mean (ms) |
| -----: | -----------------: | --------------------: |
|     10 |              0.120 |                 0.065 |
|    100 |              1.055 |                 0.583 |
|  1 000 |             11.36  |                 6.41  |
| 10 000 |             94.11  |                62.33  |

Both `classify` and `planCaveats` scale linearly: each 10x in tool
count produces ≈10x in latency (within run-to-run noise). Even the
10 000-tool synthetic case classifies in **< 100 ms** and plans
caveats in **< 65 ms** — comfortably inside the SPEC's 1-second
budget for "Classify + report: O(tools), < 1 second".

## SPEC.md target tracking

| Target (from SPEC.md)                                       | Status                                         |
| ----------------------------------------------------------- | ---------------------------------------------- |
| Classify + report: O(tools), < 1 second                     | **Confirmed.** Linear; 1 000 tools in ~18 ms.  |
| Memory: < 256 MB on 100-tool server                         | Not measured here (no allocation profile yet). |
| Enumerate < 5 s, Fuzz < 60 s on 10-tool server              | Not measured (I/O-bound, transport-dependent). |

The "Enumerate < 5 s" and "Fuzz < 60 s" targets describe end-to-end
timings against a live MCP server and are dominated by transport
latency / fuzz budget; they are not part of this pure-CPU bench
suite. The next step would be a `bench:e2e` task that scripts a
local MCP server (e.g., `@modelcontextprotocol/server-memory`) and
times `enumerate` + `fuzz` against it.

## Methodology notes

- The classify/render/planCaveats benches use the published fixture
  documents in `examples/public-servers/server-*/{inventory,fuzz,
  classification}.json` — exactly the same inputs as `npm run -w
  @mcp-recon/cli test`, so any future change in those fixtures is
  picked up.
- The synthetic-scale generator interleaves 8 archetypes
  (read/write/delete/exec/fetch/email/charge/info) so classify hits
  every rule path; it isn't a degenerate `unknown`-only fast lane.
- We don't include `enumerate` or `fuzz` here because both are
  transport-bound. A live-server e2e bench would require spawning
  reference servers; that's deferred to a future job.
- Numbers will vary across machines; treat the numbers above as the
  baseline for this commit. Compare new runs against
  `bench/results/baseline.json` to detect regressions.

## Reproducing

```sh
git checkout master
npm install
npm run bench --workspace=@mcp-recon/cli
```

Each run writes `bench/results/v<git-sha>.json` and updates
`bench/results/latest.json`. To establish a new baseline, copy
`latest.json` over `baseline.json` and commit.
