// The browser's device preview: the page lays out at the device's CSS size, scaled down only as
// it is drawn, so WebKit's minimum font size never inflates small text (it did under a page
// zoom: 12 px text came out 20.46 px at 44 %). The page sees the device's pixel ratio, and a
// click on the device size's × goes back to Responsive.

import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import path from "node:path";

import { RUN, sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { windowState } from "../lib/sawit.js";

/** The browser tab's state, as `app_state` reports it. */
async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

/** The backlog's acceptance check: a 12/16 px paragraph's computed size, the root's, the window's. */
const MEASURE = `
const p = document.createElement('p');
p.style.cssText = 'font-size:12px;line-height:16px';
p.textContent = 'x';
document.body.append(p);
const c = getComputedStyle(p);
const r = { fs: c.fontSize, lh: c.lineHeight, dpr: devicePixelRatio,
            root: getComputedStyle(document.documentElement).fontSize,
            w: innerWidth, h: innerHeight };
p.remove();
return r;`;

async function measure(ctx) {
  const r = await ctx.call({ cmd: "browser", op: { op: "evaluate", script: MEASURE } });
  return r.value;
}

/** Opens the browser on the fixture's small-text page. */
async function openText(ctx) {
  const url = `${ctx.site.url}text.html`;
  await ctx.call({ cmd: "open_page", page: "browser" });
  await ctx.call({ cmd: "browser", op: { op: "navigate", url } });
  await until(
    async () => {
      const b = await browser(ctx);
      return b?.url === url && !b.loading && b.title === "Small Text" ? b : null;
    },
    { timeoutMs: 20_000, what: "the small-text page to load" },
  );
}

/** Where this run keeps the spec's screenshots, kept whether the test passes or fails. */
async function shotsDir(name) {
  const dir = path.join(RUN.artifacts, "device-shots", name);
  await mkdir(dir, { recursive: true });
  return dir;
}

async function settledScale(ctx, last) {
  return until(
    async () => {
      const v = (await browser(ctx))?.viewport;
      return v && v.scale !== last ? v : null;
    },
    { what: "the device's scale to follow the window" },
  );
}

sawitTest("device: iPhone 15 lays out at 393 × 852 with its text unclamped, at any panel size", { site: true, timeout: 240_000 }, async (ctx) => {
  const { win } = ctx;
  const dir = await shotsDir("iphone");
  await openText(ctx);
  await ctx.call({ cmd: "browser", op: { op: "resize", device: "iPhone 15" } });
  await win.waitFor({ role: "button", name: /^Device Size: iPhone 15 · / });
  let scale = null;
  // The window fills the screen first, then two smaller sizes: each draws the phone smaller.
  for (const [i, size] of [null, [1100, 760], [820, 600]].entries()) {
    if (size) await win.resize(...size);
    const v = await settledScale(ctx, scale);
    scale = v.scale;
    const r = await until(async () => {
      const m = await measure(ctx);
      return m?.w === 393 && m?.h === 852 ? m : null;
    }, { what: `the page to see 393 × 852 at scale ${scale}` }).catch(async (e) => {
      ctx.log(`measured: ${JSON.stringify(await measure(ctx))}`);
      throw e;
    });
    ctx.log(`scale ${scale}: ${JSON.stringify(r)}`);
    await win.screenshot(path.join(dir, `${i}-scale-${Math.round(scale * 100)}.png`)).catch(() => {});
    assert.equal(r.fs, "12px", `font size at scale ${scale}`);
    assert.equal(r.lh, "16px", `line height at scale ${scale}`);
    assert.equal(r.root, "16px", `root font size at scale ${scale}`);
    assert.equal(r.w, 393);
    assert.equal(r.h, 852);
    assert.ok(Math.abs(r.dpr - 3) < 1e-3, `the iPhone's pixel ratio at scale ${scale}: ${r.dpr}`);
  }
  // The iPad mini, at each window size again (its height came out 1134 once).
  await ctx.call({ cmd: "browser", op: { op: "resize", device: "iPad mini" } });
  scale = null;
  for (const [i, size] of [[1100, 760], [820, 600], null].entries()) {
    if (size) await win.resize(...size);
    else await win.fill();
    const v = await settledScale(ctx, scale);
    scale = v.scale;
    const r = await until(async () => {
      const m = await measure(ctx);
      return m?.w === 744 && m?.h === 1133 ? m : null;
    }, { what: `the page to see 744 × 1133 at scale ${scale}` }).catch(async (e) => {
      ctx.log(`measured: ${JSON.stringify(await measure(ctx))}`);
      throw e;
    });
    await win.screenshot(path.join(dir, `ipad-${i}-scale-${Math.round(scale * 100)}.png`)).catch(() => {});
    assert.equal(r.fs, "12px");
    assert.ok(Math.abs(r.dpr - 2) < 1e-3, `the iPad's pixel ratio: ${r.dpr}`);
  }
  await ctx.call({ cmd: "browser", op: { op: "resize", device: "iPhone 15" } });

  // Turned on its side, the page sees the size swapped.
  await ctx.call({ cmd: "browser", op: { op: "resize", width: 852, height: 393 } });
  const side = await until(async () => {
    const m = await measure(ctx);
    return m?.w === 852 ? m : null;
  }, { what: "the page to see 852 wide" });
  assert.equal(side.h, 393);
  assert.equal(side.fs, "12px");

  // An element's screenshot is the element, at its CSS size, whatever the scale.
  const shot = await ctx.call({ cmd: "browser", op: { op: "screenshot", selector: "#title" } });
  const rect = (await ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return document.getElementById('title').getBoundingClientRect().width" } })).value;
  assert.equal(shot.width, Math.round(rect));
});

sawitTest("device: Desktop and Responsive keep the screen's pixel ratio and the page's own text", { site: true }, async (ctx) => {
  const { win } = ctx;
  await openText(ctx);
  const responsive = await measure(ctx);
  ctx.log(`responsive: ${JSON.stringify(responsive)}`);
  assert.equal(responsive.fs, "12px");
  assert.equal(responsive.root, "16px");
  const screen = responsive.dpr;

  await ctx.call({ cmd: "browser", op: { op: "resize", device: "Desktop" } });
  const desktop = await until(async () => {
    const m = await measure(ctx);
    return m?.w === 1440 ? m : null;
  }, { what: "the page to see 1440 wide" });
  ctx.log(`desktop: ${JSON.stringify(desktop)}`);
  assert.equal(desktop.fs, "12px");
  assert.ok(Math.abs(desktop.dpr - screen) < 1e-3, `a computer's preview keeps the screen's ratio: ${desktop.dpr}`);
  await win.screenshot(path.join(await shotsDir("desktop"), "desktop.png")).catch(() => {});

  // The device size's × goes back to Responsive in one click.
  await win.click({ role: "button", name: "Back to Responsive" });
  await win.waitFor({ role: "button", name: "Device Size: Responsive" });
  await win.waitForGone({ role: "button", name: "Back to Responsive" });
  const back = await until(async () => {
    const m = await measure(ctx);
    return m && m.w !== 1440 ? m : null;
  }, { what: "the page to fill the tab again" });
  assert.equal(back.w, responsive.w);
  assert.equal(back.dpr, screen);
  assert.equal(back.fs, "12px");
});
