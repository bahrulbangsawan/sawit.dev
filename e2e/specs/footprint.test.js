// Memory: Sawit's footprint at idle, with a browser tab, with the canvas open (and a Mermaid
// diagram drawn on it), with 10 terminals, and two idle minutes later, for the size-and-RAM note
// (docs/notes/2026-11-07-size-and-ram.md). It measures rather than asserts, so it runs only when
// asked for:
//
//   SAWIT_E2E_FOOTPRINT=1 SAWIT_E2E_BINARY=target/release/sawit e2e/run.sh --no-build --filter footprint
//
// Each step waits for the app to settle, then reads `footprint` (the dirty memory macOS charges
// the process) and `ps` (RSS) for Sawit, and `footprint` for the WebKit processes that started
// after it (a browser tab's web content and networking processes). The numbers go to the test's
// log and to footprint.json in its artifacts.

import { execFile } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";

import { RUN, sawitTest } from "../lib/harness.js";
import { windowState } from "../lib/sawit.js";
import { until } from "../lib/wait.js";

const run = promisify(execFile);
const SETTLE_MS = Number(process.env.SAWIT_E2E_FOOTPRINT_SETTLE_MS ?? 8_000);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** `footprint`'s total for `pid`, in MB. */
async function footprintMb(pid) {
  const { stdout } = await run("/usr/bin/footprint", ["-p", String(pid)]).catch((e) => ({ stdout: e.stdout ?? "" }));
  const m = stdout.match(/Footprint:\s*([\d.]+)\s*(KB|MB|GB|B)/);
  if (!m) return null;
  const scale = { B: 1 / 1048576, KB: 1 / 1024, MB: 1, GB: 1024 }[m[2]];
  return Math.round(Number(m[1]) * scale * 10) / 10;
}

/** RSS of `pid`, in MB. */
async function rssMb(pid) {
  const { stdout } = await run("/bin/ps", ["-o", "rss=", "-p", String(pid)]).catch(() => ({ stdout: "" }));
  return Math.round((Number(stdout.trim()) / 1024) * 10) / 10;
}

/** The pids of WebKit's helper processes running now. */
async function webkitPids() {
  const { stdout } = await run("/usr/bin/pgrep", ["-f", "com.apple.WebKit"]).catch(() => ({ stdout: "" }));
  return new Set(stdout.split("\n").filter(Boolean).map(Number));
}

const footprintTest = process.env.SAWIT_E2E_FOOTPRINT === "1" ? {} : { pending: "set SAWIT_E2E_FOOTPRINT=1 to measure" };
// With SAWIT_E2E_FOOTPRINT_STACKS=1, Sawit records where it allocates (MallocStackLogging) and each
// step also saves the biggest allocations with their stacks (malloc_history): slower, for finding
// what a step's memory is.
const STACKS = process.env.SAWIT_E2E_FOOTPRINT_STACKS === "1";

sawitTest(
  "footprint: memory at idle, with a browser tab, the canvas and 10 terminals",
  { ...footprintTest, site: true, timeout: 1_200_000, env: () => (STACKS ? { MallocStackLogging: "1" } : {}) },
  async (ctx) => {
    const before = await webkitPids();
    const out = path.join(RUN.artifacts, "footprint");
    const rows = [];
    const measure = async (step, settleMs = SETTLE_MS) => {
      await sleep(settleMs);
      // The canvas renders and frees frames for a while after a change: the least of three
      // samples, a second apart, is what the step keeps.
      const samples = [];
      for (let i = 0; i < 3; i++) {
        if (i) await sleep(1_000);
        samples.push(await footprintMb(ctx.app.pid));
      }
      const ours = [...(await webkitPids())].filter((p) => !before.has(p));
      const webkit = (await Promise.all(ours.map(footprintMb))).reduce((a, b) => a + (b ?? 0), 0);
      const row = {
        step,
        footprint_mb: Math.min(...samples.filter((v) => v != null)),
        footprint_max_mb: Math.max(...samples.filter((v) => v != null)),
        rss_mb: await rssMb(ctx.app.pid),
        webkit_mb: Math.round(webkit * 10) / 10,
        webkit_processes: ours.length,
      };
      rows.push(row);
      ctx.log(JSON.stringify(row));
      // The whole breakdown (by memory category) beside the numbers, to see what grew.
      const detail = await run("/usr/bin/footprint", ["-p", String(ctx.app.pid)]).catch((e) => ({ stdout: e.stdout ?? "" }));
      await mkdir(out, { recursive: true });
      await writeFile(path.join(out, `${rows.length}-${step.replace(/\W+/g, "-")}.txt`), detail.stdout);
      if (STACKS) {
        const history = await run("/usr/bin/malloc_history", [String(ctx.app.pid), "-allBySize"], { maxBuffer: 1 << 30 }).catch((e) => ({ stdout: e.stdout ?? "" }));
        await writeFile(path.join(out, `${rows.length}-stacks.txt`), history.stdout.slice(0, 2_000_000));
        const vmmap = await run("/usr/bin/vmmap", ["--summary", String(ctx.app.pid)], { maxBuffer: 1 << 26 }).catch((e) => ({ stdout: e.stdout ?? "" }));
        await writeFile(path.join(out, `${rows.length}-vmmap.txt`), vmmap.stdout);
      }
    };

    await measure("idle");

    await ctx.call({ cmd: "open_page", page: "browser" });
    await ctx.call({ cmd: "browser", op: { op: "navigate", url: ctx.site.url } });
    await until(
      async () => {
        const b = (await windowState(ctx)).browser;
        return b && !b.loading && b.url?.startsWith(ctx.site.url) ? b : null;
      },
      { timeoutMs: 30_000, what: "the page to load" },
    ).catch((e) => ctx.log(`browser: ${e.message}`));
    await measure("one browser tab");

    await ctx.call({ cmd: "open_page", page: "canvas" });
    await until(async () => ((await windowState(ctx)).canvases ?? []).some((c) => c.open), {
      what: "the canvas",
    });
    await measure("canvas open");

    await ctx.call({ cmd: "canvas", op: { op: "mermaid", source: "flowchart LR\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Done]\n  B -->|no| A", x: 0, y: 0 } });
    await measure("canvas + a Mermaid diagram");

    const count = async () => (await windowState(ctx)).terminals.length;
    while ((await count()) < 10) {
      // A reply now and then comes back empty while tabs are opening fast: the count decides.
      await ctx.call({ cmd: "terminal_run", command: "true", new_tab: true }).catch((e) => ctx.log(`terminal_run: ${e.message}`));
      await sleep(300);
    }
    await until(async () => (await count()) >= 10, { what: "10 terminals" });
    await measure("10 terminals");

    // Pele's engine is given back two minutes after the last diagram (crates/core/src/mermaid/pele.rs).
    await measure("two idle minutes later", 125_000);

    await writeFile(path.join(out, "footprint.json"), JSON.stringify(rows, null, 2));
    ctx.log(`written to ${out}/footprint.json`);
  },
);
