// Annotate in a device preview (headed: a real click in the page). The click lands on the element
// under the pointer at the preview's scale, and the note's popover keeps its size on screen and
// fits the phone (its layout is checked by eye in the screenshots it leaves). Typing and Return
// save the note on the element clicked (the second paragraph: mesurer's toolbar sits over the
// heading at Desktop size).

import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import path from "node:path";

import { RUN, sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { windowState } from "../lib/sawit.js";

/** A browser op; an empty reply (the app dropped the op's answer, seen now and then right after
 * the tab opens) is tried again. */
async function op(ctx, body) {
  for (let i = 0; ; i++) {
    try {
      return await ctx.call({ cmd: "browser", op: body });
    } catch (e) {
      if (i >= 2 || !/unexpected reply/.test(e.message)) throw e;
      await new Promise((r) => setTimeout(r, 500));
    }
  }
}

async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

/** The device's view on screen, from the toolbar's buttons: the page area runs from the
 * toolbar's left to the Annotate panel (340 pt at its right), and from the toolbar's foot to the
 * terminal dock; the device is framed in it as the browser frames it (16 pt around, a 24 pt
 * caption above, centered). */
async function deviceFrame(ctx, css) {
  const back = await ctx.win.waitFor({ role: "button", name: "Back" }, { bounds: true });
  const inspector = await ctx.win.waitFor({ role: "button", name: "Web Inspector" }, { bounds: true });
  const terminals = await ctx.win.waitFor({ role: "tablist", name: "Terminals" }, { bounds: true });
  const x0 = back.bounds.x - 8;
  const x1 = inspector.bounds.x + inspector.bounds.width + 8 - 340;
  const y0 = back.bounds.y + back.bounds.height / 2 + 18;
  const y1 = terminals.bounds.y - 1;
  const { scale } = (await browser(ctx)).viewport;
  const w = css.width * scale;
  const h = css.height * scale;
  const areaW = x1 - x0 - 32;
  const areaH = y1 - y0 - 32 - 24;
  return { x: x0 + 16 + (areaW - w) / 2, y: y0 + 24 + 16 + (areaH - h) / 2, scale };
}

for (const device of ["iPhone 15", "iPad mini", "Desktop"]) {
  sawitTest(`annotate-device: a click on ${device} annotates the element under it, in a popover that fits`, { site: true, headed: true }, async (ctx) => {
    const { win } = ctx;
    const dir = path.join(RUN.artifacts, "annotate-shots");
    await mkdir(dir, { recursive: true });
    const url = `${ctx.site.url}text.html`;
    await ctx.call({ cmd: "open_page", page: "browser" });
    await op(ctx, { op: "navigate", url });
    await until(async () => (await browser(ctx))?.url === url && !(await browser(ctx)).loading, { timeoutMs: 20_000, what: "the page" });
    const size = await op(ctx, { op: "resize", device });
    ctx.log(`resize: ${JSON.stringify(size)}`);
    await win.click({ role: "switch", name: "Annotate" });
    await until(async () => (await browser(ctx)).pick_mode === "annotate", { what: "Annotate to pick" });
    // The overlay is drawn once the page has settled (a second after load).
    await new Promise((r) => setTimeout(r, 1500));
    const vp = (await browser(ctx)).viewport;
    const rect = (await op(ctx, { op: "evaluate", script: "const r = document.querySelector('p.s14').getBoundingClientRect(); return {x: r.x + 40, y: r.y + r.height / 2}" })).value;
    const frame = await deviceFrame(ctx, vp);
    const at = { x: Math.round(frame.x + rect.x * frame.scale), y: Math.round(frame.y + rect.y * frame.scale) };
    ctx.log(`frame ${JSON.stringify(frame)} click ${JSON.stringify(at)}`);
    await ctx.desktop.call(["mouse-click", "--xy", `${at.x},${at.y}`], { headed: true });
    await new Promise((r) => setTimeout(r, 800));
    const slug = device.replace(/\W+/g, "-");
    await win.screenshot(path.join(dir, `${slug}-popover.png`)).catch(() => {});
    for (const ch of "fits") await win.pressInPage(ch);
    // An agent points at the element under the popover: its pill must never cover the popover.
    await op(ctx, { op: "hover", selector: "p.s14" }).catch((e) => ctx.log(`hover: ${e.message}`));
    await new Promise((r) => setTimeout(r, 900));
    await win.screenshot(path.join(dir, `${slug}-agent.png`)).catch(() => {});
    // Return saves the note (a second Return if the first took macOS's text suggestion).
    const saved = async () => {
      const r = await op(ctx, { op: "annotations" });
      return r.count > 0 ? r : null;
    };
    await win.pressInPage("return");
    let notes = await until(saved, { timeoutMs: 3_000, what: "the note" }).catch(() => null);
    if (!notes) {
      await win.pressInPage("return");
      notes = await until(saved, { what: "the note to be saved" });
    }
    ctx.log(`notes: ${notes.markdown}`);
    assert.match(notes.markdown, /s14/);
    assert.match(notes.markdown, /fits/i);
  });
}
