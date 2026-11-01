// `sawitTest`: one node:test test with a Sawit of its own. Before the body runs: a temp world
// (home, Sawit data folder, fixture project), Sawit started in it, and its window found. After it,
// whatever happened: Sawit killed by its pid, the fixture site closed, the app the person had in
// front brought back, and the temp world removed, its artifacts kept first when the test failed
// (or with --keep).

import { cp, mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { beforeEach, test } from "node:test";

import { launch } from "./app.js";
import { Desktop } from "./desktop.js";
import { makeProject, makeWorld, serveSite } from "./fixture.js";
import { activate, frontApp, idleSeconds, movePointer, pointer, waitForIdle } from "./host.js";

/** The tests that start no Sawit (pending, or skipped without --headed): no wait before them. */
const NO_APP = new Set();

/** What run.sh hands each test file, through the environment. */
export const RUN = {
  id: process.env.SAWIT_E2E_RUN_ID || `adhoc-${Date.now()}`,
  artifacts: process.env.SAWIT_E2E_ARTIFACTS || path.resolve(import.meta.dirname, "..", "artifacts", "adhoc"),
  headed: process.env.SAWIT_E2E_HEADED === "1",
  keep: process.env.SAWIT_E2E_KEEP === "1",
  // A test does not start while someone is using the Mac (0 turns the check off).
  idleS: Number(process.env.SAWIT_E2E_IDLE ?? 60),
  agentDesktopHome: process.env.AGENT_DESKTOP_HOME,
};

/**
 * Declares a test. `opts`:
 * - `headed`: the test moves the real pointer (a drag), so it runs only with `--headed`;
 * - `site`: serve the fixture project's `site/` folder on a loopback port (`ctx.site.url`);
 * - `args`: Sawit's arguments after `--foreground` (default: the fixture project);
 * - `pending`: a reason; the test is reported as a TODO and its body does not run;
 * - `timeout`: ms for the whole test, setup and teardown included.
 *
 * The body gets `ctx`: `{ world, app, win, desktop, site, call, log }`, where `call` sends a
 * control request to this test's Sawit and `win` drives its window (see desktop.js).
 */
export function sawitTest(name, opts, body) {
  if (typeof opts === "function") [opts, body] = [{}, opts];
  const { headed = false, site = false, args, pending, timeout = 180_000 } = opts;
  if (pending) {
    NO_APP.add(name);
    return test(name, { todo: pending }, () => {});
  }
  if (headed && !RUN.headed) {
    NO_APP.add(name);
    return test(name, { skip: "moves the real pointer: run with --headed on an idle Mac" }, () => {});
  }
  return test(name, { timeout }, async (t) => {
    const front = await frontApp().catch(() => null);
    // A test that moves the pointer puts it back where it found it.
    const pointerAt = headed ? await pointer().catch(() => null) : null;
    const world = await makeWorld();
    await makeProject(world);
    const log = (line) => t.diagnostic(line);
    const ctx = { world, log, site: null, app: null, win: null, desktop: null };
    let failed = false;
    try {
      if (site) ctx.site = await serveSite(world);
      ctx.app = await launch(world, { args: args?.(world) });
      // Sawit asks to come forward as it starts, and macOS may refuse it while another app has
      // just taken the front (the one the last test's teardown or macOS itself brought back).
      // A Sawit window that has never been in front stays out of accessibility's reach, so the
      // harness brings it forward itself.
      await activate(ctx.app.pid);
      ctx.call = (request, o) => ctx.app.call(request, o);
      ctx.desktop = new Desktop({
        env: agentDesktopEnv(),
        logFile: path.join(world.artifacts, "agent-desktop.log"),
        headed: RUN.headed,
      });
      ctx.win = ctx.desktop.window(await ctx.desktop.windowOf(ctx.app.pid));
      await ctx.win.fill().catch((e) => log(`the window kept its size: ${e.message}`));
      await body(ctx);
    } catch (e) {
      failed = true;
      await collect(ctx).catch((err) => log(`collecting artifacts failed: ${err.message}`));
      throw e;
    } finally {
      await ctx.app?.kill().catch(() => {});
      await ctx.site?.close().catch(() => {});
      // Under run.sh the app that was in front comes back once, when the run ends, rather than
      // between tests, where it would only flicker up before the next Sawit.
      if (front?.pid && !process.env.SAWIT_E2E_RUN_ID) await activate(front.pid);
      if (pointerAt) await movePointer(pointerAt);
      if (failed || RUN.keep) {
        const dest = path.join(RUN.artifacts, slug(name));
        await mkdir(dest, { recursive: true });
        await cp(world.artifacts, dest, { recursive: true });
        // The project as the test left it: what the app wrote is often the clue.
        await cp(world.project, path.join(dest, "project"), { recursive: true }).catch(() => {});
        log(`artifacts: ${dest}`);
      }
      // Its shells may still be writing their history as they hang up: a few tries.
      await rm(world.root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    }
  });
}

/** The environment agent-desktop runs with: its state in this run's folder, not ~/.agent-desktop. */
function agentDesktopEnv() {
  const env = { ...process.env };
  if (RUN.agentDesktopHome) env.AGENT_DESKTOP_HOME = RUN.agentDesktopHome;
  return env;
}

/**
 * Waits while someone is using the Mac: a test brings Sawit to the front and types into it,
 * which would take their keyboard. run.sh waits for the full idle period before the run; each
 * test waits again for a shorter one, outside its own time limit, and fails after 10 minutes.
 */
beforeEach(
  async (t) => {
    if (!RUN.idleS || NO_APP.has(t.name)) return;
    const need = Math.min(RUN.idleS, 15);
    if ((await idleSeconds()) >= need) return;
    t.diagnostic(`waiting for ${need} s without keyboard or mouse input (key presses of the test before count too)`);
    await waitForIdle(need, { timeoutS: 600 });
  },
  { timeout: 620_000 },
);

/** Keeps what explains a failure: a screenshot, the accessibility tree and the app's state. */
async function collect(ctx) {
  const dir = ctx.world.artifacts;
  if (ctx.win) {
    await ctx.win.screenshot(path.join(dir, "window.png")).catch(() => {});
    const snap = await ctx.win.snapshot().catch((e) => ({ error: e.message }));
    await writeFile(path.join(dir, "ax-tree.json"), JSON.stringify(snap.raw ?? snap, null, 1));
  }
  if (ctx.app?.socket) {
    const state = await ctx.app.call({ cmd: "state" }).catch((e) => ({ error: e.message }));
    await writeFile(path.join(dir, "app-state.json"), JSON.stringify(state, null, 1));
  }
}

function slug(name) {
  return name.replace(/[^a-z0-9]+/gi, "-").replace(/^-|-$/g, "").toLowerCase();
}
