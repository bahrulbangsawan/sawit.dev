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

const TALL = `<!doctype html><html><head><meta charset="utf-8"><title>Tall</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>body{margin:0;font-family:-apple-system,sans-serif}header{position:sticky;top:0;height:60px;background:#e11d48;color:#fff}
.hero{height:100vh;background:#fde68a}section{height:1000px;border-bottom:4px solid #111}</style></head>
<body><header>Sticky header</header><div class="hero">A 100vh hero</div>
<section>1</section><section>2</section><section>3</section><img loading="lazy" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10'/%3E"></body></html>`;

sawitTest("capture: full page comes out whole at the device's width, as PNG and PDF", { site: true }, async (ctx) => {
  const { writeFile } = await import("node:fs/promises");
  const path = await import("node:path");
  await writeFile(path.join(ctx.world.project, "site", "tall.html"), TALL);
  await openSite(ctx, "tall.html");
  const before = (await browser(ctx)).viewport;
  const r = await capture(ctx, { full_page: true, device: "iPhone 15", scale: 2 });
  // 60 header + 852 hero (100vh held at the phone's height) + 3 × 1000 sections.
  assert.equal(r.width, 786);
  assert.ok(r.height >= 2 * 3900 && r.height <= 2 * 3940, `height ${r.height}`);
  assert.deepEqual(await pngSize(r.path), [r.width, r.height]);
  assert.match(r.path, /-iphone-15-2x-full\.png$/);
  const pdf = await capture(ctx, { full_page: true, device: "Laptop", format: "pdf" });
  const head = (await readFile(pdf.path)).toString("ascii", 0, 4);
  assert.equal(head, "%PDF");
  assert.deepEqual((await browser(ctx)).viewport, before, "the tab's size is back");
});
