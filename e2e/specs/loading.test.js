// The browser shows a page loading: a progress bar under the toolbar, and Stop in Reload's place,
// for as long as the page takes, so a slow page never looks hung. Both go once it has loaded.

import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import path from "node:path";

import { RUN, sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { windowState } from "../lib/sawit.js";

async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

sawitTest("loading: a slow page shows progress and Stop until it has loaded", { site: true }, async (ctx) => {
  const { win, site } = ctx;
  const dir = path.join(RUN.artifacts, "loading-shots");
  await mkdir(dir, { recursive: true });
  await ctx.call({ cmd: "open_page", page: "browser" });
  await ctx.call({ cmd: "browser", op: { op: "navigate", url: site.url } });
  await until(async () => (await browser(ctx))?.url === site.url && !(await browser(ctx)).loading, {
    timeoutMs: 20_000,
    what: "the fixture site to load",
  });
  await win.waitFor({ role: "button", name: "Reload" });

  // The user opens a page that takes six seconds to come.
  const slow = `${site.url}slow?ms=6000`;
  await win.setValue({ role: "textfield", name: "Search or enter address" }, slow);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.pressInPage("return");
  const bar = await win.waitFor({ role: /progress/i, name: "Loading the page" }, { timeoutMs: 5_000 });
  ctx.log(`progress bar: ${JSON.stringify(bar)}`);
  await win.waitFor({ role: "button", name: "Stop" }, { timeoutMs: 2_000 });
  assert.equal((await browser(ctx)).loading, true);
  await win.screenshot(path.join(dir, "loading.png")).catch(() => {});

  // Loaded: the bar goes and Reload comes back.
  await until(async () => (await browser(ctx)).title === "Slow Page" && !(await browser(ctx)).loading, {
    timeoutMs: 15_000,
    what: "the slow page to finish loading",
  });
  await win.waitForGone({ role: /progress/i, name: "Loading the page" }, { timeoutMs: 5_000 });
  await win.waitFor({ role: "button", name: "Reload" });
  await win.screenshot(path.join(dir, "loaded.png")).catch(() => {});
});
