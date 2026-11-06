// A panel filling the window: the terminal, the browser and the canvas each fill it with the
// keyboard inside them (the browser's page holding it too), with ⇧⌘↩ and with their own ⌃⌘1/2/3,
// and the same key brings the layout back exactly as it was. `app_state` says which panel fills
// the window (`maximized`) and the layout.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas, terminalText, windowState } from "../lib/sawit.js";

/** The layout and the panel filling the window, as `app_state` reports them. */
async function shown(ctx) {
  const w = await windowState(ctx);
  return { layout: w.layout, maximized: w.maximized ?? null };
}

/** Waits until `panel` (or nothing, for `null`) fills the window, and returns what shows. */
async function waitFilled(ctx, panel) {
  return until(
    async () => {
      const s = await shown(ctx);
      return s.maximized === panel ? s : null;
    },
    { what: panel ? `the ${panel} to fill the window` : "the layout to come back" },
  );
}

/** Opens the browser on the fixture site by typing its address: Return gives the page the keyboard. */
async function openSite(ctx) {
  const { win, site } = ctx;
  await ctx.call({ cmd: "open_page", page: "browser" });
  await win.setValue({ role: "textfield", name: "Search or enter address" }, site.url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.pressInPage("return");
  await until(
    async () => {
      const b = (await windowState(ctx)).browser;
      return b?.url === site.url && !b.loading ? b : null;
    },
    { timeoutMs: 20_000, what: "the fixture site to load" },
  );
}

/** The page's keys since it loaded (the fixture site logs them). */
async function pageKeys(ctx) {
  const r = await ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return window.__keys" } });
  return r?.value ?? [];
}

sawitTest("fullscreen: the terminal fills the window and back, with its keyboard", async (ctx) => {
  const { win } = ctx;
  const before = await shown(ctx);
  assert.equal(before.maximized, null);
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), { timeoutMs: 20_000, what: "the prompt" });
  await win.focus({ role: /^text/, name: "Terminal" });
  await win.press("ctrl+cmd+1");
  const filled = await waitFilled(ctx, "terminal");
  assert.deepEqual(filled.layout, { sidebar: false, center: false, terminal: true });
  // The title bar's way back, and the status bar gone.
  await win.waitFor({ role: "button", name: "Restore Layout" });
  // The terminal still has the keyboard: what is typed runs there.
  await win.typeKeys("echo filled-1");
  await win.press("return");
  await until(async () => /^filled-1$/m.test(await terminalText(ctx)), { what: "the typed line" });
  await win.press("ctrl+cmd+1");
  assert.deepEqual((await waitFilled(ctx, null)).layout, before.layout);
  // ⇧⌘↩ with the keyboard in a one-pane terminal does the same.
  await win.press("cmd+shift+return");
  await waitFilled(ctx, "terminal");
  await win.press("cmd+shift+return");
  assert.deepEqual((await waitFilled(ctx, null)).layout, before.layout);
});

sawitTest("fullscreen: the browser fills the window and back while its page has the keyboard", { site: true }, async (ctx) => {
  const { win } = ctx;
  const before = await shown(ctx);
  await openSite(ctx);
  const opened = await shown(ctx);
  await win.pressInPage("q");
  await until(async () => (await pageKeys(ctx)).includes("q"), { what: "the page to have the keyboard" });
  await win.pressInPage("cmd+shift+return");
  const filled = await waitFilled(ctx, "browser");
  assert.deepEqual(filled.layout, { sidebar: false, center: true, terminal: false });
  // The page kept the keyboard.
  await win.pressInPage("w");
  await until(async () => (await pageKeys(ctx)).includes("w"), { what: "the page to keep the keyboard" });
  await win.pressInPage("cmd+shift+return");
  assert.deepEqual((await waitFilled(ctx, null)).layout, opened.layout);
  // ⌃⌘2 from the page too, and back.
  await win.pressInPage("ctrl+cmd+2");
  await waitFilled(ctx, "browser");
  await win.pressInPage("ctrl+cmd+2");
  assert.deepEqual((await waitFilled(ctx, null)).layout, opened.layout);
  // Escape outside the page (the address field) puts the layout back too.
  await win.pressInPage("ctrl+cmd+2");
  await waitFilled(ctx, "browser");
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.press("escape");
  assert.deepEqual((await waitFilled(ctx, null)).layout, opened.layout);
  assert.ok(before.layout.terminal, "the terminal showed before");
});

sawitTest("fullscreen: the canvas fills the window and back with its keyboard", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  const opened = await shown(ctx);
  await win.focus({ role: "canvas" }).catch(() => {});
  await win.press("cmd+shift+return");
  const filled = await waitFilled(ctx, "canvas");
  assert.deepEqual(filled.layout, { sidebar: false, center: true, terminal: false });
  await win.press("cmd+shift+return");
  assert.deepEqual((await waitFilled(ctx, null)).layout, opened.layout);
  await win.press("ctrl+cmd+3");
  await waitFilled(ctx, "canvas");
  // Restore Layout in the title bar.
  await win.click({ role: "button", name: "Restore Layout" });
  assert.deepEqual((await waitFilled(ctx, null)).layout, opened.layout);
});

sawitTest("fullscreen: one panel straight to another keeps the layout to come back to", async (ctx) => {
  const before = await shown(ctx);
  await ctx.call({ cmd: "maximize", panel: "terminal" });
  await waitFilled(ctx, "terminal");
  await ctx.call({ cmd: "maximize", panel: "canvas" });
  await waitFilled(ctx, "canvas");
  await ctx.call({ cmd: "maximize", panel: "canvas" });
  assert.deepEqual((await waitFilled(ctx, null)).layout, before.layout);
  // The old command still works.
  await ctx.call({ cmd: "maximize_terminal" });
  await waitFilled(ctx, "terminal");
  await ctx.call({ cmd: "maximize_terminal" });
  assert.deepEqual((await waitFilled(ctx, null)).layout, before.layout);
});
