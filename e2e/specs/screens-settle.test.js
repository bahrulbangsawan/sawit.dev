// Screens on the canvas are pictured whole and sharp (B17): an agent writes three mobile screens in
// a row, one with a staggered entrance animation and one revealed by timers, and each frame's
// picture shows the finished page, not a half-drawn one, at 45%, 100% and 200% zoom.

import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas } from "../lib/sawit.js";

const SESSION = { id: "mcp-e2e-settle", name: "Claude Code", label: "fixture-app" };
const FIXTURES = path.resolve(import.meta.dirname, "..", "fixtures", "screens");
const SCREENS = ["progress", "today", "session"];

const op = async (ctx, body) => {
  const started = Date.now();
  try {
    return await ctx.call({ cmd: "canvas", op: body, agent: SESSION }, { timeoutMs: 30_000 });
  } finally {
    ctx.log(`${body.op} took ${Date.now() - started} ms`);
  }
};
/** A read, tried again when the socket closes without a reply (seen under heavy load). */
async function read(ctx, request, opts) {
  for (let tries = 1; ; tries++) {
    try {
      return await ctx.call(request, opts);
    } catch (e) {
      if (tries >= 3 || !/unexpected reply/.test(String(e))) throw e;
      ctx.log(`retrying ${request.cmd} ${request.op?.op}: ${e}`);
    }
  }
}
const web = (ctx, body) => read(ctx, { cmd: "canvas", op: { op: "web", ...body } });

/** Each screen frame's state, once every one is idle with a picture. */
async function pictured(ctx, ids, minScale = 0) {
  return until(
    async () => {
      const status = await web(ctx, { action: "status" });
      const frames = (status.frames ?? []).filter((f) => ids.includes(f.id));
      const done = frames.length === ids.length &&
        frames.every((f) => f.phase === "Idle" && f.texture && f.texture.points_per_css >= minScale);
      return done ? frames : null;
    },
    { timeoutMs: 45_000, what: "every screen pictured" },
  );
}

/** Saves `screen_get`'s PNG of `name` to the run's artifacts. */
async function savePicture(ctx, work, name, tag) {
  const got = await read(ctx, { cmd: "canvas", op: { op: "screen_get", work }, agent: SESSION }, { timeoutMs: 30_000 });
  assert.equal(got.ok, true, JSON.stringify(got).slice(0, 400));
  if (got.png_base64) {
    const dir = path.join(ctx.world.artifacts, "pictures");
    await mkdir(dir, { recursive: true });
    await writeFile(path.join(dir, `${name}-${tag}.png`), Buffer.from(got.png_base64, "base64"));
  }
  return got;
}

sawitTest("screens: three screens written in a row are pictured whole and sharp", async (ctx) => {
  await openCanvas(ctx);
  const works = [];
  for (const [i, name] of SCREENS.entries()) {
    const begun = await op(ctx, {
      op: "begin", name: `${name[0].toUpperCase()}${name.slice(1)} / Mobile`, kind: "screen",
      frame: { x: i * 433, y: 0, width: 393, height: 852 },
    });
    assert.equal(begun.ok, true, JSON.stringify(begun));
    const html = await readFile(path.join(FIXTURES, `${name}.html`), "utf8");
    const wrote = await op(ctx, { op: "screen_write", work: begun.work, html });
    assert.equal(wrote.ok, true, JSON.stringify(wrote));
    works.push({ name, work: begun.work, frame: Number(wrote.frame ?? begun.frame) });
  }
  const ids = works.map((w) => w.frame);
  for (const zoom of [0.45, 1, 2]) {
    // The view centred on each screen in turn at this zoom, so every one is on screen when its
    // picture is checked.
    for (const w of works) {
      const i = SCREENS.indexOf(w.name);
      await web(ctx, { action: "view", zoom, x: i * 433 + 196, y: 426 });
      const frames = await pictured(ctx, [w.frame], zoom >= 1 ? zoom * 0.95 : 0);
      const frame = frames[0];
      ctx.log(`${w.name} at ${zoom}: ${JSON.stringify(frame.texture)} drawn=${JSON.stringify(frame.drawn)}`);
      // The picture shows the page as it ends: the half-drawn check finds no flat band where the
      // page has content.
      assert.ok(frame.drawn?.whole !== false, `${w.name} at ${zoom} is whole: ${JSON.stringify(frame.drawn)}`);
      await savePicture(ctx, w.work, w.name, `${zoom}`);
      await ctx.win.screenshot(path.join(ctx.world.artifacts, `window-${w.name}-${zoom}.png`)).catch(() => {});
    }
  }
  const status = await web(ctx, { action: "status" });
  await writeFile(path.join(ctx.world.artifacts, "web-status.json"), JSON.stringify(status, null, 2));
  assert.equal(ids.length, 3);
});
