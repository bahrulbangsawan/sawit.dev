// Menus over the browser's page. The page is a WebKit view above the window's drawing, so a menu
// opened over it (the footer's Pages menu, the account menu) would open unseen behind it: while one
// covers the page, the page steps back behind a picture of itself, and comes back once it closes.

import assert from "node:assert/strict";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas, openPage, waitForTab, windowState } from "../lib/sawit.js";

/** Where to keep a picture of the window with each menu open, when asked (`SAWIT_E2E_SHOTS`). */
const SHOTS = process.env.SAWIT_E2E_SHOTS;

async function shoot(win, name) {
  if (SHOTS) await win.screenshot(path.join(SHOTS, name));
}

/** The browser tab's state, as `app_state` reports it. */
async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

/** Waits until the page is covered and stepped back (`covered`), or up and uncovered. */
async function pageIs(ctx, covered, what) {
  return until(
    async () => {
      const b = await browser(ctx);
      return b && b.covered === covered && b.page_shown === !covered ? b : null;
    },
    { timeoutMs: 10_000, what },
  );
}

sawitTest("covers: the Pages and account menus open over the browser's page, which comes back", { site: true }, async (ctx) => {
  const { win, site } = ctx;
  await openPage(ctx, "Browser");
  await waitForTab(ctx, "browser");
  await win.setValue({ role: "textfield", name: "Search or enter address" }, site.url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.press("return");
  await until(
    async () => {
      const b = await browser(ctx);
      return b?.url === site.url && !b.loading ? b : null;
    },
    { timeoutMs: 20_000, what: "the fixture site to load" },
  );
  await pageIs(ctx, false, "the page to show, with nothing over it");

  // The footer's Pages menu rises over the page's bottom right.
  await win.openMenu({ role: "button", name: "Pages" });
  await win.waitFor({ role: "menuitem", name: "Canvas" });
  await pageIs(ctx, true, "the page to step back under the Pages menu");
  await shoot(win, "pages-menu-over-the-browser.png");
  await win.press("escape");
  await win.waitForGone({ role: "menuitem", name: "Canvas" });
  await pageIs(ctx, false, "the page to come back once the menu closed");

  // The title bar's account menu drops over the page's top right.
  await win.openMenu({ role: "button", name: /GitHub/ });
  await win.waitFor({ role: "menuitem", name: "Canvas" });
  await pageIs(ctx, true, "the page to step back under the account menu");
  await shoot(win, "account-menu-over-the-browser.png");
  await win.press("escape");
  await win.waitForGone({ role: "menuitem", name: "Canvas" });
  const back = await pageIs(ctx, false, "the page to come back after the account menu");
  assert.equal(back.url, site.url, "the page is the one it was");
});

/** The canvas's live web frame, as its `web` op reports it: `{id, placed, hidden}` and whether
 * something covers the canvas (`covered`), or null. */
async function live(ctx) {
  const status = await ctx.call({ cmd: "canvas", op: { op: "web", action: "status" } });
  return status?.live ? { ...status.live, covered: status.covered } : null;
}

sawitTest("covers: the Pages menu opens over the canvas's live web frame, which comes back", { site: true }, async (ctx) => {
  const { win, site } = ctx;
  await openCanvas(ctx);
  const laid = await ctx.call({
    cmd: "canvas",
    op: { op: "routes", server: site.url, routes: ["/"], viewports: [1280] },
  });
  assert.equal(laid.ok, true, JSON.stringify(laid));
  const frame = laid.frames[0];
  const id = typeof frame === "object" ? frame.id : frame;
  // A frame goes live once its page has its first picture.
  await until(
    async () => {
      await ctx.call({ cmd: "canvas", op: { op: "web", action: "enter", id } });
      const now = await live(ctx);
      ctx.log(`live: ${JSON.stringify(now)}`);
      return now?.id === Number(id) && now.placed && !now.hidden ? now : null;
    },
    { timeoutMs: 45_000, what: "the route frame to go live" },
  );
  await win.openMenu({ role: "button", name: "Pages" });
  await win.waitFor({ role: "menuitem", name: "Canvas" });
  await until(async () => ((await live(ctx))?.hidden ? true : null), {
    timeoutMs: 10_000,
    what: "the live view to step back under the Pages menu",
  });
  await shoot(win, "pages-menu-over-a-live-frame.png");
  await win.press("escape");
  await win.waitForGone({ role: "menuitem", name: "Canvas" });
  await until(
    async () => {
      const now = await live(ctx);
      return now?.id === Number(id) && !now.hidden ? now : null;
    },
    { timeoutMs: 10_000, what: "the live view to come back once the menu closed" },
  );
});
