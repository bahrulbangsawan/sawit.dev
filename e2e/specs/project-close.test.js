// Closing a project closes its terminals (backlog item 12): a server started in the project's
// terminal stops with it, its port frees up, and no shell is left behind. The question lists
// what would stop.

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { windowState } from "../lib/sawit.js";

const run = promisify(execFile);

/** Whether something listens on TCP `port` (lsof exits 1 when nothing does). */
async function listening(port) {
  try {
    await run("/usr/sbin/lsof", ["-nP", `-iTCP:${port}`, "-sTCP:LISTEN"]);
    return true;
  } catch {
    return false;
  }
}

/** Whether a process whose command line has `pattern` runs. */
async function runs(pattern) {
  try {
    await run("/usr/bin/pgrep", ["-f", pattern]);
    return true;
  } catch {
    return false;
  }
}

sawitTest("project close: its terminals close, and the server they started stops", async (ctx) => {
  const { win } = ctx;
  const project = path.basename(ctx.world.project);
  const port = 40000 + (process.pid % 20000);
  // A server in the project's own terminal (a new tab in its folder), as `bun dev` would be.
  const server = `nc -lk 127.0.0.1 ${port}`;
  await ctx.call({ cmd: "terminal_run", command: server, cwd: ctx.world.project });
  await until(() => listening(port), { what: `the server to listen on ${port}` });
  const tabs = (await windowState(ctx)).terminals.filter((t) => t.project === ctx.world.project);
  assert.ok(tabs.length > 0, "the project has terminals");

  await win.click({ role: "button", name: `Close ${project}` });
  const go = { role: "button", name: "Close and Stop" };
  await win.waitFor(go);
  await win.click(go);
  await until(async () => !(await listening(port)), { timeoutMs: 15_000, what: "the port to free up" });
  await until(async () => !(await runs(server)), { what: "the server to be gone" });
  const left = (await windowState(ctx)).terminals.filter((t) => t.project === ctx.world.project);
  assert.equal(left.length, 0, "a terminal of the closed project is still open");
});
