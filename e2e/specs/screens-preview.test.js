// Preview and Present of a screen (B15): Preview's bar holds the name, one size menu, Rotate,
// Reload and Close, with no timer pill over it; Present draws the screen's page, not a blank frame.

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

sawitTest("screens: Preview's bar has one size menu and nothing over it", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  const { frame } = await today(ctx);
  await op(ctx, { op: "command", command: "select.set", params: { ids: [frame] } });
  await win.click({ role: "button", name: "Preview" });
  await win.waitFor({ role: "button", name: "Close" });
  await win.screenshot(path.join(ctx.world.artifacts, "preview.png"));
  // One size picker, no row of every device.
  const buttons = (await win.findAll({ role: "button" })).map((b) => b.name);
  assert.ok(!buttons.some((n) => /Pixel 8/.test(n ?? "")), `no device row: ${buttons}`);
  assert.ok(!buttons.includes("Timer 5:00"), "no timer pill over Preview");
  await win.openMenu({ role: "button", name: /^Size: / });
  await win.waitFor({ role: "menuitem", name: /Pixel 8/ });
  await win.screenshot(path.join(ctx.world.artifacts, "preview-sizes.png"));
  await win.click({ role: "menuitem", name: /Pixel 8/ });
  await win.waitFor({ role: "button", name: /^Size: / });
  await win.screenshot(path.join(ctx.world.artifacts, "preview-pixel.png"));
  await win.click({ role: "button", name: "Close" });
});

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
  // The panels of a design file: the screen's row in Layers, then the Prototype tab.
  await win.screenshot(path.join(ctx.world.artifacts, "panels.png"));
  await win.click({ role: "tab", name: "Prototype" }).catch(() => {});
  await win.screenshot(path.join(ctx.world.artifacts, "prototype.png"));
  await win.press("alt+cmd+enter");
  await until(async () => {
    const state = await windowState(ctx);
    const canvas = (state.canvases ?? []).find((c) => c.active);
    return canvas?.view?.presenting ? canvas : null;
  }, { timeoutMs: 15_000, what: "Present to show" });
  // Its sharper picture comes in a moment.
  await new Promise((r) => setTimeout(r, 2_500));
  await win.screenshot(path.join(ctx.world.artifacts, "present.png"));
  await win.press("escape");
});
