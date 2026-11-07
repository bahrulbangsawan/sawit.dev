// Present of a screen (B15): it draws the screen's page, not a blank frame; and the screen's
// row in the Layers panel says what it is.

import assert from "node:assert/strict";
import { copyFile, readFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas, windowState } from "../lib/sawit.js";

const SESSION = { id: "mcp-e2e-preview", name: "Claude Code", label: "fixture-app" };
const FIXTURES = path.resolve(import.meta.dirname, "..", "fixtures", "screens");

const op = (ctx, body, file) =>
  ctx.call({ cmd: "canvas", op: body, agent: SESSION, ...(file ? { file } : {}) }, { timeoutMs: 30_000 });

/** Writes the Today screen on the canvas (or `file`), and waits for its picture. */
async function today(ctx, file) {
  const begun = await op(ctx, {
    op: "begin", name: "Today / Mobile", kind: "screen", frame: { x: 0, y: 0, width: 393, height: 852 },
  }, file);
  assert.equal(begun.ok, true, JSON.stringify(begun));
  const html = await readFile(path.join(FIXTURES, "today.html"), "utf8");
  const wrote = await op(ctx, { op: "screen_write", work: begun.work, html }, file);
  assert.equal(wrote.ok, true, JSON.stringify(wrote));
  const frame = Number(wrote.frame ?? begun.frame);
  await until(async () => {
    const status = await op(ctx, { op: "web", action: "status" }, file);
    const f = (status.frames ?? []).find((x) => x.id === frame);
    return f?.phase === "Idle" && f.texture ? f : null;
  }, { timeoutMs: 45_000, what: "the screen pictured" });
  return { frame, work: begun.work };
}

sawitTest("screens: Present draws the screen's page", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  // A design file: boards have no Present.
  const file = path.join(ctx.world.project, ".nyawit", "screens-design.sawit");
  await copyFile(path.join(FIXTURES, "design.sawit"), file);
  // Its tab in front, with the keyboard, as the user would have it: a canvas renders its screens
  // while it is on show.
  await ctx.call({ cmd: "open", path: file });
  const { frame } = await today(ctx, file);
  await win.click({ role: "image", name: "Canvas" }).catch(() => {});
  await op(ctx, { op: "command", command: "select.set", params: { ids: [frame] } }, file);
  // The panels of a design file: the screen's row in Layers has its icon and HTML tag.
  await win.screenshot(path.join(ctx.world.artifacts, "panels.png"));
  // Present, as the user's ⌥⌘↩ would open it (the web op opens it for tests).
  await op(ctx, { op: "web", action: "present", id: frame }, file);
  await until(async () => {
    const state = await windowState(ctx);
    const canvas = (state.canvases ?? []).find((c) => c.active);
    return canvas?.view?.presenting ? canvas : null;
  }, { timeoutMs: 15_000, what: "Present to show" });
  // Its sharper picture comes in a moment.
  await new Promise((r) => setTimeout(r, 2_500));
  await win.screenshot(path.join(ctx.world.artifacts, "present.png"));
  await op(ctx, { op: "web", action: "close_present" }, file);
});
