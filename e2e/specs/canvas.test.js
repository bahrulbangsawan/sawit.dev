// The canvas: its toolbar picks a tool, a drag draws with it, and the drawing is saved in the
// project's .nyawit/canvas.sawit.

import assert from "node:assert/strict";
import { stat } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas } from "../lib/sawit.js";

/** The canvas's layers, as its own `get` op returns them. */
async function scene(ctx) {
  return JSON.stringify(await ctx.call({ cmd: "canvas", op: { op: "get" } }));
}

sawitTest("canvas: the toolbar picks the rectangle tool", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  // The tools are switches: one is on, the active tool (Move, at first).
  await win.waitFor({ role: "switch", name: "Move", within: "Canvas tools", value: true });
  await win.click({ role: "switch", name: "Rectangle", within: "Canvas tools" });
  await win.waitFor({ role: "switch", name: "Rectangle", within: "Canvas tools", value: true });
  const on = await win.findAll({ role: "switch", within: "Canvas tools", value: true });
  assert.deepEqual(
    on.map((n) => n.name),
    ["Rectangle"],
    "only the rectangle tool is on",
  );
});

sawitTest(
  "canvas: the toolbar's rectangle tool draws through the canvas's pointer, saved in .nyawit/canvas.sawit",
  async (ctx) => {
    const { win, world } = ctx;
    await openCanvas(ctx);
    await win.click({ role: "switch", name: "Rectangle", within: "Canvas tools" });
    await win.waitFor({ role: "switch", name: "Rectangle", within: "Canvas tools", value: true });
    // The drag itself goes through the canvas's own pointer input (its `pointer` op, the path a
    // mouse drag takes after GPUI), so no real pointer moves: the tool is the one the toolbar
    // picked. The test below does it with the real pointer.
    const at = (kind, x, y) => ctx.call({ cmd: "canvas", op: { op: "pointer", kind, x, y } });
    await at("down", 100, 100);
    await at("move", 220, 180);
    await at("move", 340, 260);
    const drawn = await at("up", 340, 260);
    assert.equal(drawn.ok, true, JSON.stringify(drawn));
    assert.equal(drawn.selection.length, 1, "the new rectangle is selected");

    const text = await until(async () => {
      const s = await scene(ctx);
      return /rect/i.test(s) ? s : null;
    }, { what: "a rectangle on the canvas" });
    assert.match(text, /rect/i);
    const file = path.join(world.project, ".nyawit", "canvas.sawit");
    await until(async () => (await stat(file).catch(() => null))?.size > 0, {
      what: `${file} to be written`,
    });
  },
);

sawitTest(
  "canvas: a drag with the rectangle tool draws a rectangle, saved in .nyawit/canvas.sawit",
  // The drag is a real one: the pointer moves.
  { headed: true },
  async (ctx) => {
    const { win, world } = ctx;
    await openCanvas(ctx);
    await win.click({ role: "switch", name: "Rectangle", within: "Canvas tools" });
    await win.waitFor({ role: "switch", name: "Rectangle", within: "Canvas tools", value: true });

    // Where the canvas is on screen, from its accessibility node; the drag stays clear of the
    // toolbar along its foot.
    const canvas = await win.waitFor({ role: "image", name: "Canvas" }, { bounds: true });
    const { x, y, width, height } = canvas.bounds;
    const from = { x: x + width * 0.35, y: y + height * 0.25 };
    const to = { x: x + width * 0.55, y: y + height * 0.5 };
    await win.drag(from, to);

    const text = await until(async () => {
      const s = await scene(ctx);
      return /rect/i.test(s) ? s : null;
    }, { what: "a rectangle on the canvas" });
    assert.match(text, /rect/i);
    const file = path.join(world.project, ".nyawit", "canvas.sawit");
    await until(async () => (await stat(file).catch(() => null))?.size > 0, {
      what: `${file} to be written`,
    });
  },
);
