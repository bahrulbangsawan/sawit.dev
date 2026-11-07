// Convert to Layers (B15): an agent's HTML screen becomes a frame of editable layers beside it,
// with text layers, auto layout and its colours as variables; the screen stays as "… / HTML", and
// one undo takes the conversion back.

import assert from "node:assert/strict";
import { copyFile, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas } from "../lib/sawit.js";

const SESSION = { id: "mcp-e2e-layers", name: "Claude Code", label: "fixture-app" };
const FIXTURES = path.resolve(import.meta.dirname, "..", "fixtures", "screens");

const op = (ctx, body, file) =>
  ctx.call({ cmd: "canvas", op: body, agent: SESSION, file }, { timeoutMs: 45_000 });

sawitTest("screens: Convert to Layers makes an editable frame beside the screen", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  const file = path.join(ctx.world.project, ".nyawit", "layers-design.sawit");
  await copyFile(path.join(FIXTURES, "design.sawit"), file);
  await ctx.call({ cmd: "open", path: file });
  const begun = await op(ctx, {
    op: "begin", name: "Today / Mobile", kind: "screen", frame: { x: 0, y: 0, width: 393, height: 852 },
  }, file);
  assert.equal(begun.ok, true, JSON.stringify(begun));
  const html = await readFile(path.join(FIXTURES, "today.html"), "utf8");
  const wrote = await op(ctx, { op: "screen_write", work: begun.work, html }, file);
  assert.equal(wrote.ok, true, JSON.stringify(wrote));
  const frame = Number(begun.frame);
  await until(async () => {
    const status = await op(ctx, { op: "web", action: "status" }, file);
    const f = (status.frames ?? []).find((x) => x.id === frame);
    return f?.phase === "Idle" && f.texture ? f : null;
  }, { timeoutMs: 45_000, what: "the screen pictured" });

  const made = await op(ctx, { op: "screen_layers", screen: "today-mobile" }, file);
  await writeFile(path.join(ctx.world.artifacts, "converted.json"), JSON.stringify(made, null, 2));
  assert.equal(made.ok, true, JSON.stringify(made));
  assert.ok(made.layers >= 10, `layers: ${made.layers}`);
  assert.ok(made.texts >= 4, `texts: ${made.texts}`);
  assert.ok(made.auto_layouts >= 1, `auto layouts: ${made.auto_layouts}`);
  assert.equal(made.name, "Today / Mobile");

  // The new frame holds layers, text among them, that can be selected one by one.
  const id = Number(made.frame);
  const node = await op(ctx, { op: "command", command: "node.get", params: { id, outline: true } }, file);
  assert.equal(node.ok, true, JSON.stringify(node).slice(0, 400));
  assert.ok((node.result.children ?? []).length >= 1, "layers inside");
  const texts = await op(ctx, { op: "command", command: "node.find", params: { type: "TEXT" } }, file);
  assert.ok((texts.result?.ids ?? []).length >= made.texts, JSON.stringify(texts).slice(0, 300));
  const html_frame = await op(ctx, { op: "command", command: "node.get", params: { id: frame } }, file);
  assert.match(JSON.stringify(html_frame.result), /Today \/ Mobile \/ HTML/);

  // Both side by side, then the new frame selected, for the eye.
  await op(ctx, { op: "web", action: "view", zoom: 0.8, x: 450, y: 430 }, file);
  await new Promise((r) => setTimeout(r, 1_500));
  await win.screenshot(path.join(ctx.world.artifacts, "side-by-side.png"));
  await op(ctx, { op: "command", command: "select.set", params: { ids: [id] } }, file);
  await new Promise((r) => setTimeout(r, 500));
  await win.screenshot(path.join(ctx.world.artifacts, "selected.png"));

  // One undo takes it back, the screen's name too.
  await op(ctx, { op: "command", command: "edit.undo" }, file);
  const after = await op(ctx, { op: "command", command: "node.get", params: { id: frame } }, file);
  assert.doesNotMatch(JSON.stringify(after.result), /\/ HTML/);
  const left = await op(ctx, { op: "command", command: "node.find", params: { type: "TEXT" } }, file);
  assert.equal((left.result?.ids ?? []).length, 0, `the converted layers are gone after undo: ${JSON.stringify(left)}`);
});
