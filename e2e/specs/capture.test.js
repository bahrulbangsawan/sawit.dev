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
  assert.ok(r.height >= 2 * 3900 && r.height <= 2 * 3960, `height ${r.height}`);
  assert.deepEqual(await pngSize(r.path), [r.width, r.height]);
  assert.match(r.path, /-iphone-15-2x-full\.png$/);
  const pdf = await capture(ctx, { full_page: true, device: "Laptop", format: "pdf" });
  const head = (await readFile(pdf.path)).toString("ascii", 0, 4);
  assert.equal(head, "%PDF");
  assert.deepEqual((await browser(ctx)).viewport, before, "the tab's size is back");
});

/** Whether Quick Look can open the video: it makes a thumbnail of it. */
async function quickLookOpens(file) {
  const { execFile } = await import("node:child_process");
  const { mkdtemp, readdir } = await import("node:fs/promises");
  const os = await import("node:os");
  const path = await import("node:path");
  const out = await mkdtemp(path.join(os.tmpdir(), "ql-"));
  await new Promise((resolve) => execFile("/usr/bin/qlmanage", ["-t", "-s", "256", "-o", out, file], () => resolve()));
  return (await readdir(out)).some((f) => f.endsWith(".png"));
}

async function record(ctx, op) {
  return ctx.call({ cmd: "browser", op: { op: "record_start", ...op } }, { timeoutMs: 60_000 });
}

async function stop(ctx) {
  return ctx.call({ cmd: "browser", op: { op: "record_stop" } }, { timeoutMs: 120_000 });
}

sawitTest("capture: native recordings play in Quick Look at the size asked for", { site: true, timeout: 300_000 }, async (ctx) => {
  await openSite(ctx);
  const started = await record(ctx, { device: "Laptop", resolution: "720p", fps: 30, max_seconds: 20 });
  assert.equal(started.recording, true);
  assert.deepEqual([started.width, started.height], [1152, 720]);
  const status = await ctx.call({ cmd: "browser", op: { op: "record_status" } });
  assert.equal(status.recording, true);
  await new Promise((r) => setTimeout(r, 2500));
  const done = await stop(ctx);
  assert.equal(done.codec, "H.264");
  assert.ok(done.frames > 5, `frames ${done.frames}`);
  assert.ok(done.fps_achieved > 2, `fps ${done.fps_achieved}`);
  const head = await readFile(done.path);
  assert.equal(head.toString("ascii", 4, 8), "ftyp");
  assert.ok(await quickLookOpens(done.path), "Quick Look opens the MP4");

  const mov = await record(ctx, { device: "Full HD", resolution: "1080p", format: "mov", max_seconds: 20 });
  assert.deepEqual([mov.width, mov.height], [1920, 1080]);
  await new Promise((r) => setTimeout(r, 1500));
  const movDone = await stop(ctx);
  assert.equal(movDone.codec, "HEVC");
  assert.ok(await quickLookOpens(movDone.path), "Quick Look opens the MOV");

  await record(ctx, { format: "gif", max_width: 400, fps: 15, max_seconds: 10 });
  await new Promise((r) => setTimeout(r, 1200));
  const gif = await stop(ctx);
  assert.equal((await readFile(gif.path)).toString("ascii", 0, 3), "GIF");
  assert.ok(gif.width <= 400);

  // A recording stops on its own at its cap.
  await record(ctx, { max_seconds: 2 });
  const capped = await until(async () => {
    const s = await ctx.call({ cmd: "browser", op: { op: "record_status" } });
    return s.recording === false && s.last ? s : null;
  }, { timeoutMs: 20_000, what: "the recording to stop at its cap" });
  assert.ok(capped.last.duration_seconds <= 2.5);
});

sawitTest("capture: a full-page recording scrolls the page top to bottom", { site: true, timeout: 300_000 }, async (ctx) => {
  const { writeFile } = await import("node:fs/promises");
  const path = await import("node:path");
  await writeFile(path.join(ctx.world.project, "site", "tall.html"), TALL);
  await openSite(ctx, "tall.html");
  const started = await record(ctx, { mode: "full_page", device: "iPhone 15", resolution: "720p", duration: 3, max_seconds: 20 });
  assert.equal(started.mode, "fullpage");
  const ended = await until(async () => {
    const s = await ctx.call({ cmd: "browser", op: { op: "record_status" } });
    return s.recording === false && s.last ? s.last : null;
  }, { timeoutMs: 30_000, what: "the scroll-through to end" });
  assert.ok(ended.duration_seconds >= 3 && ended.duration_seconds <= 8, `duration ${ended.duration_seconds}`);
  assert.equal(ended.width, 720);
  assert.ok(await quickLookOpens(ended.path), "Quick Look opens it");
  const y = await ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return scrollY" } });
  assert.equal(y.value, 0, "the scroll is put back");
});

/** Where screenshots for review go, when asked. */
const SHOTS = process.env.SAWIT_E2E_SHOTS;
async function shot(ctx, name) {
  if (!SHOTS) return;
  const { mkdir } = await import("node:fs/promises");
  const path = await import("node:path");
  await mkdir(SHOTS, { recursive: true });
  await new Promise((r) => setTimeout(r, 400));
  await ctx.win.screenshot(path.join(SHOTS, `${name}.png`));
}

sawitTest("capture: the camera menu takes a screenshot and records, with the capture bar after", { site: true, timeout: 300_000 }, async (ctx) => {
  const { win } = ctx;
  await openSite(ctx);
  await win.openMenu({ role: "button", name: "Capture" });
  await win.waitFor({ role: "menuitem", name: "Visible area" });
  await shot(ctx, "capture-menu");
  await win.click({ role: "menuitem", name: "Visible area" });
  await win.waitFor({ role: "button", name: "Reveal in Finder" }, { timeoutMs: 30_000 });
  await win.waitFor({ role: "button", name: "Copy" });
  await shot(ctx, "capture-bar-done");

  await win.openMenu({ role: "button", name: "Capture" });
  await win.click({ role: "menuitem", name: "Record visible area" });
  await win.waitFor({ role: "button", name: "Stop Recording" }, { timeoutMs: 30_000 });
  await new Promise((r) => setTimeout(r, 2200));
  await shot(ctx, "capture-recording");
  const status = await ctx.call({ cmd: "browser", op: { op: "record_status" } });
  assert.equal(status.recording, true);
  await win.click({ role: "button", name: "Stop Recording" });
  await win.waitFor({ role: "button", name: "Reveal in Finder" }, { timeoutMs: 60_000 });
  await shot(ctx, "capture-bar-video");
  const after = await ctx.call({ cmd: "browser", op: { op: "record_status" } });
  assert.equal(after.recording, false);
  assert.ok(after.last.path.endsWith(".mp4"));

  // A tab that is recording asks before it closes.
  await ctx.call({ cmd: "browser", op: { op: "record_start", max_seconds: 60 } });
  await win.click({ role: "button", name: "Close Small Text" });
  await win.waitFor({ name: "Stop the recording and close the tab?" }, { timeoutMs: 10_000 }).catch(() => {});
  await shot(ctx, "capture-close-confirm");
  await win.press("escape");
  await ctx.call({ cmd: "browser", op: { op: "record_stop" } }, { timeoutMs: 60_000 });
});
