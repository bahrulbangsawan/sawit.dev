// The panels and Preview of a screen (B15): the Design panel shows only what applies to an HTML
// screen (its Screen section, no Fill or Auto layout), a converted text shows its type first, and
// Preview's bar sizes, zooms, frames and takes a custom size; at a narrow window too. Screenshots
// go to the run's artifacts for the design review.

import assert from "node:assert/strict";
import { copyFile, readFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas, windowState } from "../lib/sawit.js";

const SESSION = { id: "mcp-e2e-panels", name: "Claude Code", label: "fixture-app" };
const FIXTURES = path.resolve(import.meta.dirname, "..", "fixtures", "screens");

const op = (ctx, body, file) =>
  ctx.call({ cmd: "canvas", op: body, agent: SESSION, file }, { timeoutMs: 45_000 });
const web = (ctx, body, file) => op(ctx, { op: "web", ...body }, file);
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const shot = (ctx, name) => ctx.win.screenshot(path.join(ctx.world.artifacts, `${name}.png`));

async function previewState(ctx) {
  const state = await windowState(ctx);
  return (state.canvases ?? []).find((c) => c.active)?.view?.preview_state;
}

sawitTest("screens: the panels fit a screen and Preview sizes, zooms and frames it", async (ctx) => {
  await openCanvas(ctx);
  const file = path.join(ctx.world.project, ".nyawit", "panels-design.sawit");
  await copyFile(path.join(FIXTURES, "design.sawit"), file);
  await ctx.call({ cmd: "open", path: file });
  const begun = await op(ctx, {
    op: "begin", name: "Today / Mobile", kind: "screen", frame: { x: 0, y: 0, width: 393, height: 852 },
  }, file);
  assert.equal(begun.ok, true, JSON.stringify(begun));
  const html = await readFile(path.join(FIXTURES, "today.html"), "utf8");
  assert.equal((await op(ctx, { op: "screen_write", work: begun.work, html }, file)).ok, true);
  const frame = Number(begun.frame);
  await until(async () => {
    const status = await web(ctx, { action: "status" }, file);
    const f = (status.frames ?? []).find((x) => x.id === frame);
    return f?.phase === "Idle" && f.texture ? f : null;
  }, { timeoutMs: 45_000, what: "the screen pictured" });

  // A screen selected: its panel.
  await op(ctx, { op: "command", command: "select.set", params: { ids: [frame] } }, file);
  await pause(600);
  await shot(ctx, "screen-panel");

  // Converted, and one of its texts selected: Typography leads.
  const made = await op(ctx, { op: "screen_layers", screen: "today-mobile" }, file);
  assert.equal(made.ok, true, JSON.stringify(made));
  const found = await op(ctx, { op: "command", command: "node.find", params: { type: "TEXT" } }, file);
  const texts = found.result?.ids ?? [];
  ctx.log(`texts: ${JSON.stringify(found).slice(0, 200)}`);
  if (texts.length) {
    await op(ctx, { op: "command", command: "select.set", params: { ids: [Number(texts[0])] } }, file);
    await pause(600);
    await shot(ctx, "text-panel");
  }

  // Preview.
  await web(ctx, { action: "preview", id: frame }, file);
  await until(async () => (await previewState(ctx))?.placed ? true : null, { timeoutMs: 15_000, what: "Preview" });
  await pause(1_500);
  await shot(ctx, "preview-fit");
  let state = await web(ctx, { action: "preview_set", frame: true }, file);
  assert.equal(state.result.device_frame, true, JSON.stringify(state));
  await pause(1_000);
  await shot(ctx, "preview-frame");
  state = await web(ctx, { action: "preview_set", zoom: "100%", frame: false }, file);
  await pause(800);
  const actual = await previewState(ctx);
  assert.equal(actual.zoom, "100%");
  assert.equal(actual.placed[4], 1, `one CSS px a point: ${JSON.stringify(actual)}`);
  await shot(ctx, "preview-actual");
  await web(ctx, { action: "preview_set", size: "Desktop", zoom: "fit" }, file);
  await pause(1_000);
  await shot(ctx, "preview-desktop");
  // Custom size: a wrong one says what to type; a right one lays the page out at it.
  await web(ctx, { action: "preview_custom", w: "50", h: "768" }, file);
  await pause(300);
  await web(ctx, { action: "preview_custom", w: "50", h: "768", apply: true }, file);
  await pause(600);
  await shot(ctx, "preview-custom-error");
  assert.equal((await previewState(ctx)).custom_open, true, "still typing after a wrong size");
  await web(ctx, { action: "preview_custom", w: "1024", h: "768", apply: true }, file);
  await pause(1_000);
  const custom = await previewState(ctx);
  assert.equal(custom.size, "custom 1024x768", JSON.stringify(custom));
  assert.deepEqual(custom.css, [1024, 768]);
  await shot(ctx, "preview-custom");

  // A narrow window: the bar still fits, with nothing over it.
  await web(ctx, { action: "preview_set", size: "iPhone 15" }, file);
  await ctx.win.resize(900, 700);
  await pause(1_500);
  await shot(ctx, "preview-narrow");
  await web(ctx, { action: "close_preview" }, file);
  await pause(500);
  await op(ctx, { op: "command", command: "select.set", params: { ids: [frame] } }, file);
  await pause(600);
  await shot(ctx, "screen-panel-narrow");
});
