// mesurer in the browser's Annotate: Annotate mounts it on the page, its keys reach it from the
// page, Esc unwinds it before Sawit stops picking, what it draws stays across a reload, and closing
// Annotate takes it off the page. With --headed, real clicks: a click writes a Sawit note while no
// mesurer tool is on, none while Inspect is, and places a guide while Guides is.

import assert from "node:assert/strict";
import { copyFile, readFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openPage, waitForTab, windowState } from "../lib/sawit.js";

const FIXTURE = path.resolve(import.meta.dirname, "..", "fixtures", "mesurer", "index.html");
const HOST = "mesurer-extension-host";

/** The browser tab's state, as `app_state` reports it. */
async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

/** mesurer's `state` op: mounted, summary (tool, minimized, counts), chrome, page. */
async function mesurer(ctx) {
  return ctx.call({ cmd: "browser", op: { op: "mesurer", action: "state" } });
}

/** Runs `script` (a function body) in the page's own world; answers its value. */
async function page(ctx, script) {
  const r = await ctx.call({ cmd: "browser", op: { op: "evaluate", script } });
  return r?.value;
}

/** Waits until mesurer's summary passes `test`; answers the state. */
async function summaryUntil(ctx, test, what) {
  return until(
    async () => {
      const s = await mesurer(ctx);
      return s.mounted && test(s.summary) ? s : null;
    },
    { timeoutMs: 10_000, what },
  );
}

/** Waits until mesurer is mounted on the page (its tree is there: `ready`). */
async function mounted(ctx) {
  return until(
    async () => {
      const s = await mesurer(ctx);
      return s.mounted && s.ready ? s : null;
    },
    { timeoutMs: 20_000, what: "mesurer to be mounted" },
  );
}

/**
 * Opens the fixture page in the browser and presses Return in the address field, which also
 * gives the page the keyboard; then opens Annotate (the `panel` op, which leaves the keyboard
 * where it is) and waits for mesurer.
 */
async function openFixture(ctx) {
  const { win, site, world } = ctx;
  await copyFile(FIXTURE, path.join(world.project, "site", "mesurer.html"));
  const url = `${site.url}mesurer.html`;
  await openPage(ctx, "Browser");
  await waitForTab(ctx, "browser");
  await win.setValue({ role: "textfield", name: "Search or enter address" }, url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.press("return");
  await until(
    async () => {
      const b = await browser(ctx);
      return b?.url === url && !b.loading && b.title === "Mesurer Fixture" ? b : null;
    },
    { timeoutMs: 20_000, what: "the fixture page to load" },
  );
  await ctx.call({ cmd: "browser", op: { op: "panel", panel: "annotate" } });
  return mounted(ctx);
}

/** Sawit's picker: `annotate` while a click writes a note, `off` once Esc stopped it. */
async function picking(ctx) {
  return (await browser(ctx)).pick_mode;
}

/**
 * Esc, as the user presses it in the page. mesurer takes two Escs within a second for "minimize
 * now", so each waits that out first: the steps below are one Esc each.
 */
async function escape(ctx) {
  await new Promise((r) => setTimeout(r, 1_200));
  await ctx.win.pressInPage("escape");
}

sawitTest("mesurer: Annotate mounts it, its keys reach it, Esc unwinds it, a reload keeps it", { site: true }, async (ctx) => {
  const { win } = ctx;
  const first = await openFixture(ctx);
  assert.equal(first.version, "0.2.7");
  assert.equal(await picking(ctx), "annotate");

  // On the page: the host, kept out of Sawit's picking and snapshots, one step under Sawit's
  // overlay; mesurer's code and its bridge stay in their own world.
  const host = await page(
    ctx,
    `const h = document.getElementById("${HOST}");
     return { host: !!h, overlay: !!h && h.hasAttribute("data-sawit-overlay"), z: h && getComputedStyle(h).zIndex,
       SawitMesurer: typeof SawitMesurer, bundle: typeof __sawitMesurerBundle };`,
  );
  assert.deepEqual(host, { host: true, overlay: true, z: "2147483646", SawitMesurer: "undefined", bundle: "undefined" });

  // X: X-ray outlines the page; Esc turns it off and leaves Sawit picking.
  await win.pressInPage("x");
  await until(async () => page(ctx, `return document.body.classList.contains("xray-mode")`), {
    what: "X-ray on the page",
  });
  await escape(ctx);
  await until(async () => (await page(ctx, `return document.body.classList.contains("xray-mode")`)) === false, {
    what: "Esc to turn X-ray off",
  });
  assert.equal(await picking(ctx), "annotate", "Esc left X-ray, not picking");

  // G: Guides has the pointer (the panel's status line says so); L: a layout guide, its panel open.
  await win.pressInPage("g");
  await summaryUntil(ctx, (s) => s.tool === "guides", "the Guides tool");
  await win.waitFor({ role: "status", name: "Guides · Esc to leave" });
  await win.pressInPage("l");
  await summaryUntil(ctx, (s) => s.layout_guides >= 1, "a layout guide");

  // Esc unwinds from the innermost layer: the layout guides' panel, the tool, then mesurer's
  // toolbar (minimized); only the next one stops Sawit's picking.
  await escape(ctx);
  assert.equal(await picking(ctx), "annotate", "the first Esc closed mesurer's panel");
  await escape(ctx);
  await summaryUntil(ctx, (s) => s.tool === "none", "Esc to leave the Guides tool");
  assert.equal(await picking(ctx), "annotate");
  await win.waitFor({ role: "status", name: "Click an element to add a note" });
  await escape(ctx);
  await summaryUntil(ctx, (s) => s.minimized, "Esc to minimize mesurer");
  assert.equal(await picking(ctx), "annotate");
  await escape(ctx);
  await until(async () => (await picking(ctx)) === "off", { what: "the last Esc to stop picking" });

  // A reload keeps the layout guide (and mesurer minimized); Show tools brings its toolbar back.
  await ctx.call({ cmd: "browser", op: { op: "reload" } });
  await until(async () => (await browser(ctx)).loading === false, { what: "the reload" });
  const reloaded = await summaryUntil(ctx, (s) => s.layout_guides >= 1, "the layout guide after the reload");
  assert.equal(reloaded.summary.minimized, true);
  await win.click({ role: "button", name: "Show tools" });
  await summaryUntil(ctx, (s) => !s.minimized, "Show tools to bring mesurer back");

  // ⌘, opens mesurer's settings; their Website link goes to the default browser (kept in the
  // app's log under the harness), never to the tab.
  await win.pressInPage("cmd+comma");
  const website = `const r = document.getElementById("${HOST}").shadowRoot;
    const b = [...r.querySelectorAll("[data-mesurer-settings-panel] button")]
      .find((b) => b.textContent.trim() === "mesurer.dev");
    if (b) b.click();
    return !!b;`;
  await until(async () => page(ctx, website), { what: "mesurer's settings, with their Website link" });
  const log = path.join(ctx.world.artifacts, "app.log");
  await until(async () => /mesurer's link https:\/\/mesurer\.dev/.test(await readFile(log, "utf8")), {
    what: "the Website link to reach the app",
  });
  assert.equal((await browser(ctx)).url.endsWith("/mesurer.html"), true, "the tab stays on the page");
  await escape(ctx);

  // ⌘A: the page's Select All while mesurer's Select tool is off; mesurer's while it is on.
  const selected = () => page(ctx, "return getSelection().toString().length");
  await page(ctx, "getSelection().removeAllRanges(); return true");
  await win.pressInPage("cmd+a");
  await until(async () => (await selected()) > 0, { what: "⌘A to select the page's text" });
  await page(ctx, "getSelection().removeAllRanges(); return true");
  await win.pressInPage("s");
  await summaryUntil(ctx, (s) => s.tool === "selection", "the Select tool");
  await win.pressInPage("cmd+a");
  await new Promise((r) => setTimeout(r, 800));
  assert.equal(await selected(), 0, "⌘A in mesurer's Select tool is mesurer's");
  await escape(ctx);
  await summaryUntil(ctx, (s) => s.tool === "none", "Esc to leave the Select tool");

  // Annotate closed and opened again on the same page: mesurer's keys still reach it (B, which
  // its keyboard gate, now ahead of it, doesn't bridge).
  await ctx.call({ cmd: "browser", op: { op: "panel" } });
  await until(async () => (await mesurer(ctx)).mounted === false, { what: "mesurer to go" });
  await ctx.call({ cmd: "browser", op: { op: "panel", panel: "annotate" } });
  await mounted(ctx);
  const previews = `return document.getElementById("${HOST}").shadowRoot.querySelectorAll("[data-mesurer-responsive]").length`;
  await win.pressInPage("b");
  await until(async () => (await page(ctx, previews)) > 0, { what: "B to open Responsive again" });
  await win.pressInPage("b");
  await until(async () => (await page(ctx, previews)) === 0, { what: "B to close it" });

  // Annotate off: mesurer leaves the page.
  await ctx.call({ cmd: "browser", op: { op: "panel" } });
  await until(async () => (await page(ctx, `return !document.getElementById("${HOST}")`)) === true, {
    what: "mesurer's host to go",
  });
  assert.equal((await mesurer(ctx)).mounted, false);
});

/** Where the page is on the screen: the pointer moved there once, read back by the page. */
async function pageOrigin(ctx) {
  const windows = await ctx.desktop.call(["list-windows"]);
  const mine = (Array.isArray(windows) ? windows : (windows.windows ?? [])).find((w) => w.id === ctx.win.id);
  assert.ok(mine?.bounds, "the window's bounds");
  const b = mine.bounds;
  for (const [fx, fy] of [[0.3, 0.6], [0.25, 0.5], [0.4, 0.7]]) {
    const x = Math.round(b.x + b.width * fx);
    const y = Math.round(b.y + b.height * fy);
    await ctx.desktop.call(["mouse-move", "--xy", `${x},${y}`], { headed: true });
    const at = await until(async () => page(ctx, "return window.__pointer || null"), {
      timeoutMs: 2_000,
      what: "the page to see the pointer",
    }).catch(() => null);
    if (Array.isArray(at)) return { x: x - at[0], y: y - at[1] };
  }
  throw new Error("the pointer never reached the page");
}

/** A real click at the middle of the element `selector` (plus `dy`), on the screen. */
async function clickOn(ctx, origin, selector, dy = 0) {
  const [x, y] = await page(
    ctx,
    `const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();
     return [r.x + r.width / 2, r.y + r.height / 2];`,
  );
  const at = `${Math.round(origin.x + x)},${Math.round(origin.y + y + dy)}`;
  await ctx.desktop.call(["mouse-click", "--xy", at], { headed: true });
}

async function notes(ctx) {
  return (await ctx.call({ cmd: "browser", op: { op: "annotations" } })).count;
}

sawitTest("mesurer: a click is a Sawit note with no tool on, mesurer's with one; ⌘K copies comments", { site: true, headed: true }, async (ctx) => {
  const { win } = ctx;
  await openFixture(ctx);
  const origin = await pageOrigin(ctx);
  // Sawit's note opens on the page once the page has settled (its first second); a pick before
  // that goes to the panel's row instead.
  await new Promise((r) => setTimeout(r, 1_500));

  // No tool: the click opens Sawit's note, which Return saves.
  await clickOn(ctx, origin, "#two h2");
  for (const combo of ["h", "i", "return"]) await win.pressInPage(combo);
  await until(async () => (await notes(ctx)) === 1, { what: "a Sawit note" });

  // Inspect (I) has the pointer: a click measures, and writes no note.
  await win.pressInPage("i");
  await summaryUntil(ctx, (s) => s.tool === "select", "the Inspect tool");
  await clickOn(ctx, origin, "#three h2");
  await new Promise((r) => setTimeout(r, 800));
  assert.equal(await notes(ctx), 1, "no note while Inspect is on");

  // Guides (G): a click places a guide, kept across a reload.
  await win.pressInPage("g");
  await summaryUntil(ctx, (s) => s.tool === "guides", "the Guides tool");
  await clickOn(ctx, origin, "#one p", 40);
  await summaryUntil(ctx, (s) => s.guides === 1, "a guide");

  // Comments (M): a click opens a thread on the element, Return posts it; ⌘K (past the
  // workspace's ⌘K chords) copies the open ones for an agent, here to the harness's pasteboard.
  await win.pressInPage("m");
  await summaryUntil(ctx, (s) => s.tool === "comments", "the Comments tool");
  await clickOn(ctx, origin, "#three h2");
  for (const combo of ["h", "e", "y", "return"]) await win.pressInPage(combo);
  await summaryUntil(ctx, (s) => s.comments_open === 1, "a comment");
  await win.pressInPage("cmd+k");
  const copied = await until(async () => {
    const text = (await mesurer(ctx)).last_clipboard;
    return typeof text === "string" && text.includes("hey") ? text : null;
  }, { what: "⌘K to copy the comments for an agent" });
  assert.match(copied, /URL: http:\/\/127\.0\.0\.1:\d+\/mesurer\.html/);
  // ⇧⌘K resolves them all.
  await win.pressInPage("cmd+shift+k");
  await summaryUntil(ctx, (s) => s.comments_open === 0, "⇧⌘K to resolve the comments");
  await ctx.call({ cmd: "browser", op: { op: "reload" } });
  await until(async () => (await browser(ctx)).loading === false, { what: "the reload" });
  await summaryUntil(ctx, (s) => s.guides === 1, "the guide after the reload");
});
