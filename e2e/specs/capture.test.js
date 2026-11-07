// Native captures (Part D): the page captured as a device is laid out at the device's width and
// comes out at its pixel size, whatever the tab's size and the screen's scale; full page, PDF
// and recordings are added by the specs below as they land.

import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openPage, waitForTab, windowState } from "../lib/sawit.js";

async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

/** A PNG's pixel size, from its header. */
async function pngSize(path) {
  const b = await readFile(path);
  assert.equal(b.toString("ascii", 12, 16), "IHDR", `${path} is a PNG`);
  return [b.readUInt32BE(16), b.readUInt32BE(20)];
}

async function openSite(ctx, rel = "text.html") {
  const { win, site } = ctx;
  await openPage(ctx, "Browser");
  await waitForTab(ctx, "browser");
  const url = `${site.url}${rel}`;
  await win.setValue({ role: "textfield", name: "Search or enter address" }, url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.press("return");
  await until(async () => {
    const b = await browser(ctx);
    return b?.url === url && !b.loading ? b : null;
  }, { timeoutMs: 20_000, what: "the page" });
}

async function capture(ctx, op) {
  return ctx.call({ cmd: "browser", op: { op: "capture", ...op } }, { timeoutMs: 60_000 });
}

sawitTest("capture: each device class comes out at its own pixel size, the tab's size put back", { site: true }, async (ctx) => {
  await openSite(ctx);
  const before = (await browser(ctx)).viewport;
  for (const [device, w, h] of [
    ["iPhone 15", 1179, 2556],
    ["iPad Air", 1640, 2360],
    ["Laptop", 2560, 1600],
    ["Full HD", 1920, 1080],
  ]) {
    const r = await capture(ctx, { device });
    assert.equal(r.device, device);
    assert.deepEqual([r.width, r.height], [w, h], `${device} pixel size`);
    assert.deepEqual(await pngSize(r.path), [w, h]);
    assert.match(r.path, /-(iphone-15|ipad-air|laptop|full-hd)-[0-9_]+x\.png$/);
    assert.ok(r.preview_png_base64, "a preview for agents");
  }
  // A scale and a size of no device's, as JPEG.
  const j = await capture(ctx, { width: 500, height: 400, scale: 1, format: "jpeg", quality: 0.8 });
  assert.deepEqual([j.width, j.height], [500, 400]);
  assert.ok((await stat(j.path)).size > 0);
  // The tab's own size is back.
  assert.deepEqual((await browser(ctx)).viewport, before);
  // An element.
  const el = await capture(ctx, { selector: "#title", scale: 2 });
  assert.ok(el.width >= 2 * el.css_width - 2 && el.width <= 2 * el.css_width + 2);
});
