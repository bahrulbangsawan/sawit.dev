// One menu look and one floating bar (B16): menus open with an icon on every item and a section
// header that isn't a disabled item; the canvas's bars over a route frame, an artboard, a flow
// and a screen are as wide as their buttons and name every button. Each menu and bar is
// pictured to $SAWIT_E2E_SHOTS (else the test's artifacts) for the design review.

import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { openCanvas } from "../lib/sawit.js";

async function shotsDir(ctx) {
  const dir = process.env.SAWIT_E2E_SHOTS || ctx.world.artifacts;
  await mkdir(dir, { recursive: true });
  return dir;
}

async function command(ctx, command, params) {
  const done = await ctx.call({ cmd: "canvas", op: { op: "command", command, params } });
  assert.equal(done.ok, true, JSON.stringify(done));
  return done.result;
}

async function select(ctx, ids) {
  await command(ctx, "select.none", {});
  await command(ctx, "select.add", { ids });
}

/** The bar `name` (its buttons' names) once it shows, pictured to `file`. */
async function shotBar(ctx, dir, file, names) {
  // A toggle (Full Page) is a switch to assistive technology, with its state, not a plain button.
  for (const name of names) await ctx.win.waitFor({ role: /button|switch/, name });
  await new Promise((r) => setTimeout(r, 400));
  await ctx.win.screenshot(path.join(dir, file));
}

async function shotOpenMenu(ctx, dir, file) {
  await ctx.win.waitFor({ role: "menu" });
  await new Promise((r) => setTimeout(r, 300));
  await ctx.win.screenshot(path.join(dir, file));
  const items = await ctx.win.findAll({ role: "menuitem" });
  await ctx.win.press("escape");
  await ctx.win.waitForGone({ role: "menu" });
  return items.map((i) => i.name);
}

sawitTest("menus look: the route bar and the canvas's main menu", { timeout: 180_000 }, async (ctx) => {
  const { win } = ctx;
  const dir = await shotsDir(ctx);
  await openCanvas(ctx);
  // A route frame: Live, Reload, Full Page (a toggle) and Open in Browser.
  const routes = await ctx.call({
    cmd: "canvas",
    op: { op: "routes", server: "http://localhost:5599", routes: ["/"], x: 0, y: 0 },
  });
  assert.equal(routes.ok, true, JSON.stringify(routes));
  const frame = routes.frames[0];
  await select(ctx, [frame?.id ?? frame]);
  await shotBar(ctx, dir, "bar-route.png", ["Live", "Reload", "Full Page", "Open in Browser"]);
  const full = await win.waitFor({ role: "switch", name: "Full Page" });
  assert.ok(full.states?.includes("checked") || full.value === "true", `Full Page says it is on: ${JSON.stringify(full)}`);
  await win.openMenu({ role: "button", name: "Main menu" });
  await shotOpenMenu(ctx, dir, "menu-canvas-main.png");
});

sawitTest("menus look: the screen, artboard and flow bars (B13)", { timeout: 240_000 }, async (ctx) => {
  const { win } = ctx;
  const dir = await shotsDir(ctx);
  await openCanvas(ctx);

  // An artboard: Export and Send to Agent; two frames: Send Flow to Agent.
  const a = await command(ctx, "frame.create", { x: 0, y: 900, width: 393, height: 600, name: "Settings", props: { fill: "#ffffff" } });
  const b = await command(ctx, "frame.create", { x: 500, y: 900, width: 393, height: 600, name: "Profile", props: { fill: "#ffffff" } });
  await select(ctx, [a.id]);
  await shotBar(ctx, dir, "bar-artboard.png", ["Export", "Send to Agent"]);
  await win.openMenu({ role: "button", name: "Send to Agent" });
  const agentItems = await shotOpenMenu(ctx, dir, "menu-send-to-agent.png");
  assert.ok(agentItems.includes("Copy Prompt"), `Copy Prompt in ${agentItems}`);
  assert.ok(agentItems.includes("Reveal in Finder"), `Reveal in Finder in ${agentItems}`);
  await win.openMenu({ role: "button", name: "Export" });
  const exportItems = await shotOpenMenu(ctx, dir, "menu-export.png");
  for (const name of ["HTML", "React"]) assert.ok(exportItems.some((i) => i.startsWith(name)), `${name} in ${exportItems}`);
  await select(ctx, [a.id, b.id]);
  await shotBar(ctx, dir, "bar-flow.png", ["Send Flow to Agent"]);

  // A screen: Preview, Export and Send to Agent.
  const screen = await ctx.call({
    cmd: "canvas",
    op: { op: "begin", name: "Pricing", kind: "screen", frame: { x: 1200, y: 900, width: 390, height: 844 } },
  });
  assert.equal(screen.ok, true, JSON.stringify(screen));
  await select(ctx, [screen.frame]);
  await shotBar(ctx, dir, "bar-screen.png", ["Preview", "Export", "Send to Agent"]);
});

sawitTest("menus look: the file tree and the terminal's menus", { timeout: 120_000 }, async (ctx) => {
  const { win, world } = ctx;
  const dir = await shotsDir(ctx);
  await writeFile(path.join(world.project, "notes.md"), "# Notes\n");
  await win.rightClick({ role: "treeitem", name: "notes.md", within: "Files" });
  const fileItems = await shotOpenMenu(ctx, dir, "menu-file-tree.png");
  for (const name of ["New File", "Reveal in Finder", "Move to Trash", "Delete"]) {
    assert.ok(fileItems.includes(name), `${name} in ${fileItems}`);
  }
  await win.rightClick({ role: "radiobutton", within: "Terminals", value: true });
  await shotOpenMenu(ctx, dir, "menu-terminal-tab.png");
});
