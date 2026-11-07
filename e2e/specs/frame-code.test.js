// An artboard (a frame of layers) exports as code: the bar over it has an Export menu with HTML and
// React (B13), and the HTML, opened in Sawit's browser, lays its layers out where the canvas draws them.

import assert from "node:assert/strict";
import { writeFile, mkdir } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas, openPage, waitForTab, windowState } from "../lib/sawit.js";

/** Runs canvas engine command `command` with `params`; its result. */
async function command(ctx, command, params) {
  const done = await ctx.call({ cmd: "canvas", op: { op: "command", command, params } });
  assert.equal(done.ok, true, JSON.stringify(done));
  return done.result;
}

/** A 393 × 400 card: a column in auto layout with a title and a button, and a badge placed absolutely. */
async function drawCard(ctx) {
  const card = await command(ctx, "frame.create", {
    x: 0, y: 0, width: 393, height: 400, name: "Plan card", props: { fill: "#ffffff" },
  });
  await command(ctx, "text.create", {
    x: 24, y: 24, text: "Pro plan", parent: card.id, name: "Title",
    props: { fontSize: 24, fill: "#111827", fontFamily: "Helvetica" },
  });
  const button = await command(ctx, "frame.create", {
    x: 24, y: 80, width: 160, height: 48, parent: card.id, name: "Upgrade button",
    props: { fill: "#1d4ed8", cornerRadius: 8 },
  });
  await command(ctx, "layout.add", { ids: [card.id] });
  await command(ctx, "node.set", { ids: [card.id], props: { layout: "vertical", gap: 16, padding: 24, sizingV: "fixed", height: 400 } });
  await command(ctx, "frame.create", {
    x: 300, y: 16, width: 64, height: 24, parent: card.id, name: "Badge",
    props: { fill: "#fde68a", cornerRadius: 12, absolute: true },
  });
  await command(ctx, "select.none", {});
  await command(ctx, "select.add", { ids: [card.id] });
  return { card: card.id, button: button.id };
}

sawitTest("frame code: a selected artboard has an Export menu with HTML and React over it", async (ctx) => {
  await openCanvas(ctx);
  await drawCard(ctx);
  await ctx.win.openMenu({ role: "button", name: "Export" });
  await ctx.win.waitFor({ role: "menuitem", name: "HTML" });
  await ctx.win.waitFor({ role: "menuitem", name: "React" });
  await ctx.win.press("escape");
  await ctx.win.waitForGone({ role: "menu" });
});

sawitTest("frame code: the HTML lays the artboard out as the canvas draws it", { site: true }, async (ctx) => {
  const { world, site } = ctx;
  await openCanvas(ctx);
  const { card } = await drawCard(ctx);
  // The canvas's own boxes for the layers, from the card's corner.
  const canvasBoxes = {};
  const walk = async (id, origin) => {
    const node = await command(ctx, "node.get", { id });
    const box = [node.x - origin[0], node.y - origin[1], node.width, node.height];
    canvasBoxes[node.name] = box;
    return node;
  };
  const root = await walk(card, [0, 0]);
  canvasBoxes["Plan card"] = [0, 0, root.width, root.height];
  for (const child of root.children ?? []) {
    await walk(child.id ?? child, [0, 0]);
  }
  const code = await ctx.call({ cmd: "canvas", op: { op: "frame_code", id: String(card) } });
  assert.equal(code.ok, true, JSON.stringify(code));
  const dir = path.join(world.project, "site");
  await mkdir(dir, { recursive: true });
  await writeFile(path.join(dir, "card.html"), code.html);
  await writeFile(path.join(world.artifacts, "card.html"), code.html);
  const png = await ctx.call({ cmd: "canvas", op: { op: "export_png" } });
  if (png.png_base64) await writeFile(path.join(world.artifacts, "canvas.png"), Buffer.from(png.png_base64, "base64"));

  await openPage(ctx, "Browser");
  await waitForTab(ctx, "browser");
  const url = `${site.url}card.html`;
  await ctx.call({ cmd: "browser", op: { op: "navigate", url } });
  await until(async () => {
    const b = (await windowState(ctx)).browser;
    return b?.url === url && !b.loading ? b : null;
  }, { timeoutMs: 20_000, what: "the export to load" });
  const measured = await ctx.call({
    cmd: "browser",
    op: {
      op: "evaluate",
      script: `JSON.stringify(Object.fromEntries([...document.querySelectorAll('[data-name]')].map(e => { const r = e.getBoundingClientRect(); return [e.dataset.name, [r.x, r.y, r.width, r.height]]; })))`,
    },
  });
  const boxes = JSON.parse(measured.result ?? measured.value ?? measured);
  await writeFile(path.join(world.artifacts, "boxes.json"), JSON.stringify(boxes, null, 2));
  await writeFile(path.join(world.artifacts, "canvas-boxes.json"), JSON.stringify(canvasBoxes, null, 2));
  const close = (a, b, what) => assert.ok(Math.abs(a - b) <= 2, `${what}: ${a} in the browser, ${b} on the canvas`);
  const [cx, cy] = boxes["Plan card"];
  for (const name of ["Plan card", "Title", "Upgrade button", "Badge"]) {
    const [bx, by, bw, bh] = boxes[name];
    const [x, y, w, h] = canvasBoxes[name];
    close(bx - cx, x, `${name}'s left`);
    close(by - cy, y, `${name}'s top`);
    close(bw, w, `${name}'s width`);
    close(bh, h, `${name}'s height`);
  }
});
