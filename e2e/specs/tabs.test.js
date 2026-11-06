// Tabs in the browser (backlog item 10): three browser tabs opened, switched, closed and back after
// a restart; an agent working on a browser tab that isn't on show while the user's tab and keyboard
// stay where they are; and the memory 1, 3 and 6 tabs take.

import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import path from "node:path";

import { launch } from "../lib/app.js";
import { RUN, sawitTest } from "../lib/harness.js";
import { activate } from "../lib/host.js";
import { until } from "../lib/wait.js";
import { terminalText, waitForTerminalLine, windowState } from "../lib/sawit.js";

async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

async function tabs(ctx) {
  return (await browser(ctx))?.tabs ?? [];
}

async function shotsDir() {
  const dir = path.join(RUN.artifacts, "tabs-shots");
  await mkdir(dir, { recursive: true });
  return dir;
}

/** Restarts Sawit in the same world, as the user quitting and opening it again. */
async function restart(ctx) {
  await ctx.app.kill();
  ctx.app = await launch(ctx.world);
  await activate(ctx.app.pid);
  ctx.win = ctx.desktop.window(await ctx.desktop.windowOf(ctx.app.pid));
  await ctx.win.fill().catch(() => {});
  await ctx.win.takeFocus();
}

sawitTest("tabs: three browser tabs open, switch, close, and come back after a restart", { site: true, timeout: 300_000 }, async (ctx) => {
  const { site } = ctx;
  const dir = await shotsDir();
  const pages = [site.url, `${site.url}text.html`, `${site.url}slow?ms=100`];
  await ctx.call({ cmd: "open_page", page: "browser" });
  await ctx.call({ cmd: "browser", op: { op: "navigate", url: pages[0] } });
  await until(async () => (await tabs(ctx))[0]?.title === "Fixture Site", { timeoutMs: 20_000, what: "the first tab" });

  // Two more, as the user opens them: New Tab, then an address.
  for (const url of pages.slice(1)) {
    await ctx.win.click({ role: "button", name: "New Tab" });
    await ctx.win.setValue({ role: "textfield", name: "Search or enter address" }, url);
    await ctx.win.focus({ role: "textfield", name: "Search or enter address" });
    await ctx.win.pressInPage("return");
    await until(async () => (await tabs(ctx)).some((t) => t.url === url && !t.loading), { timeoutMs: 20_000, what: `the tab at ${url}` });
  }
  let now = await tabs(ctx);
  assert.equal(now.length, 3);
  assert.equal(now.findIndex((t) => t.active), 2, "the new tab is on show");
  await ctx.win.screenshot(path.join(dir, "three-tabs.png")).catch(() => {});

  // Switched by a click on its tab, and by ⌘1 while the browser has the keyboard.
  await ctx.win.click({ name: "Small Text" });
  await until(async () => (await tabs(ctx))[1]?.active, { what: "the second tab on show" });
  await ctx.win.focus({ role: "textfield", name: "Search or enter address" });
  await ctx.win.pressInPage("cmd+1");
  await until(async () => (await tabs(ctx))[0]?.active, { what: "⌘1 to show the first tab" });
  assert.equal((await browser(ctx)).url, pages[0]);

  // Closed with its close button.
  await ctx.win.click({ role: "button", name: "Close Small Text" });
  await until(async () => (await tabs(ctx)).length === 2, { what: "the tab to close" });

  // Back after a restart: the same tabs, the same one on show.
  await new Promise((r) => setTimeout(r, 2_500)); // the tabs are written a second after a change
  await restart(ctx);
  await ctx.call({ cmd: "open_page", page: "browser" });
  now = await until(async () => {
    const t = await tabs(ctx);
    return t.length === 2 ? t : null;
  }, { timeoutMs: 20_000, what: "the tabs to come back" });
  assert.deepEqual(now.map((t) => t.url), [pages[0], pages[2]]);
  assert.equal(now.findIndex((t) => t.active), 0);
  await until(async () => (await browser(ctx)).title === "Fixture Site", { timeoutMs: 20_000, what: "the restored page to load" });
  await ctx.win.screenshot(path.join(dir, "restored.png")).catch(() => {});
});

sawitTest("tabs: an agent works on a browser tab not on show; the user's tab and keyboard stay", { site: true, timeout: 300_000 }, async (ctx) => {
  const { site } = ctx;
  await ctx.call({ cmd: "open_page", page: "browser" });
  await ctx.call({ cmd: "browser", op: { op: "navigate", url: site.url } });
  await until(async () => (await browser(ctx))?.title === "Fixture Site", { timeoutMs: 20_000, what: "the page" });

  // The agent opens its own tab behind the user's.
  const opened = await ctx.call({ cmd: "browser_tabs", action: "open", url: `${site.url}text.html` });
  const id = String(opened.opened);
  assert.equal((await tabs(ctx)).findIndex((t) => t.active), 0, "the user's tab stays on show");

  // The user types in a terminal meanwhile.
  await ctx.win.click({ role: "button", name: "New Terminal" });
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), { timeoutMs: 20_000, what: "the prompt" });
  const term = (await windowState(ctx)).terminals.find((t) => t.active).index;

  // The agent reads and acts on its tab, off screen.
  const title = await until(async () => {
    const r = await ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return document.readyState === 'complete' && document.title" }, tab: id });
    return r.value || null;
  }, { timeoutMs: 20_000, what: "the agent's tab to load" });
  assert.equal(title, "Small Text");
  await ctx.call({ cmd: "browser", op: { op: "click", selector: "#link" }, tab: id });
  await until(async () => (await tabs(ctx)).find((t) => String(t.id) === id)?.title === "Fixture Site", { what: "the agent's click to open the link" });

  await ctx.win.typeKeys("echo user-kept");
  await ctx.win.press("return");
  await waitForTerminalLine(ctx, "user-kept", { tab: term, timeoutMs: 8_000 });
  const now = await tabs(ctx);
  assert.equal(now.findIndex((t) => t.active), 0, "the user's tab is still the one on show");
  assert.equal((await browser(ctx)).url, site.url);

  // The agent closes its tab.
  await ctx.call({ cmd: "browser_tabs", action: "close", tab: id });
  await until(async () => (await tabs(ctx)).length === 1, { what: "the agent's tab to close" });
});

/** The WebKit content processes running now: pid → RSS in KB. */
async function webContent() {
  const { execFile } = await import("node:child_process");
  const { promisify } = await import("node:util");
  const { stdout } = await promisify(execFile)("/bin/ps", ["-axo", "pid=,rss=,comm="]);
  const out = new Map();
  for (const line of stdout.split("\n")) {
    const m = line.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/);
    if (m && /WebKit\.WebContent/.test(m[3])) out.set(Number(m[1]), Number(m[2]));
  }
  return out;
}

/** Sawit's RSS and its WebKit content processes' (those started since `before`), in MB. */
async function memory(ctx, before) {
  const { execFile } = await import("node:child_process");
  const { promisify } = await import("node:util");
  const { stdout } = await promisify(execFile)("/bin/ps", ["-o", "rss=", "-p", String(ctx.app.pid)]);
  const app = Number(stdout.trim()) / 1024;
  const content = [...(await webContent())].filter(([pid]) => !before.has(pid));
  const pages = content.reduce((sum, [, rss]) => sum + rss, 0) / 1024;
  return { app: Math.round(app), pages: Math.round(pages), processes: content.length, total: Math.round(app + pages) };
}

sawitTest("tabs-memory: 1, 3 and 6 browser tabs, live and once the hidden ones gave their pages up", { site: true, timeout: 400_000 }, async (ctx) => {
  const { site } = ctx;
  const before = await webContent();
  await ctx.call({ cmd: "open_page", page: "browser" });
  await ctx.call({ cmd: "browser", op: { op: "navigate", url: `${site.url}text.html` } });
  await until(async () => (await browser(ctx))?.title === "Small Text", { timeoutMs: 20_000, what: "the first tab" });
  const settle = () => new Promise((r) => setTimeout(r, 3_000));
  await settle();
  const rows = { 1: await memory(ctx, before) };
  for (const count of [3, 6]) {
    while ((await tabs(ctx)).length < count) {
      const opened = await ctx.call({ cmd: "browser_tabs", action: "open", url: `${site.url}text.html`, show: true });
      await until(async () => (await tabs(ctx)).find((t) => t.id === opened.opened && t.title === "Small Text"), { timeoutMs: 20_000, what: "a tab to load" });
    }
    await settle();
    rows[count] = await memory(ctx, before);
  }
  // The five tabs off screen give their pages up a minute after they were last on show.
  await until(async () => (await tabs(ctx)).filter((t) => t.live).length === 1, { timeoutMs: 120_000, intervalMs: 2_000, what: "the hidden tabs to give their pages up" });
  await settle();
  rows["6, 5 suspended"] = await memory(ctx, before);
  ctx.log(`memory (MB): ${JSON.stringify(rows)}`);
  // An agent's op wakes one again, off screen, and it answers.
  const second = (await tabs(ctx))[1];
  const title = await until(async () => {
    const r = await ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return document.readyState === 'complete' && document.title" }, tab: String(second.id) });
    return r.value || null;
  }, { timeoutMs: 20_000, what: "the woken tab to answer" });
  assert.equal(title, "Small Text");
  assert.equal((await tabs(ctx)).findIndex((t) => t.active), 5, "the user's tab stays on show");
});
