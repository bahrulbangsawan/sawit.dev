// The canvas toolbar's groups and their flyouts, and canvas keys that have nothing to act on.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { openCanvas } from "../lib/sawit.js";

/** Each group's flyout (its ^ button), and the tools it lists with their keys. */
const GROUPS = [
  {
    flyout: "Text tools",
    tools: [
      ["Text", "T"],
      ["Text on a path", "⇧T"],
    ],
  },
  {
    flyout: "Frame tools",
    tools: [
      ["Frame", "F"],
      ["Section", "⇧S"],
      ["Slice", "S"],
    ],
  },
  {
    flyout: "Shape tools",
    tools: [
      ["Rectangle", "R"],
      ["Line", "L"],
      ["Arrow", "⇧L"],
      ["Ellipse", "O"],
      ["Polygon", "⇧6"],
      ["Star", "⇧8"],
      ["Place image…", "⇧⌘K"],
    ],
  },
  {
    flyout: "Drawing tools",
    tools: [
      ["Pen", "P"],
      ["Pencil", "⇧P"],
    ],
  },
];

sawitTest("canvas: the toolbar's groups each open a flyout of their tools", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  // A board has its own groups too: sticky notes, connectors and stamps.
  for (const name of ["Sticky note colors", "Connector styles", "Stamps"]) {
    await win.waitFor({ role: "button", name, within: "Canvas tools" });
  }
  for (const group of GROUPS) {
    // The ^ button opens the flyout as Return does (it has no accessibility press: see openMenu).
    await win.openMenu({ role: "button", name: group.flyout, within: "Canvas tools" });
    for (const [tool] of group.tools) await win.waitFor({ role: "menuitem", name: tool });
    // The keys beside each tool, where agent-desktop reads them (the item's description).
    const items = await win.findAll({ role: "menuitem" });
    for (const [tool, keys] of group.tools) {
      const item = items.find((i) => i.name === tool);
      if (item.description !== null) assert.equal(item.description, keys, `${tool}'s key`);
    }
    // Picking the first tool closes the flyout and makes it the active tool.
    const [first] = group.tools[0];
    await win.click({ role: "menuitem", name: first });
    await win.waitForGone({ role: "menuitem", name: group.tools[1][0] });
    await win.waitFor({ role: "switch", name: first, within: "Canvas tools", value: true });
  }
});

sawitTest("canvas: a canvas key with nothing selected does nothing, and shows no toast", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  const before = (await win.subtree({ role: "image", name: "Canvas" })).filter((n) => n.name);
  // ⌥⌘G frames the selection, and nothing is selected. A toast would show within a frame or two:
  // watching for one over 3 s is a bounded observation of something that must not happen, not a
  // sleep before a check.
  await win.press("alt+cmd+g");
  const deadline = Date.now() + 3_000;
  while (Date.now() < deadline) {
    const now = (await win.subtree({ role: "image", name: "Canvas" })).filter((n) => n.name);
    const added = now.filter((n) => !before.some((b) => b.name === n.name && b.role === n.role));
    assert.deepEqual(added.map((n) => `${n.role} ${n.name}`), [], "nothing new on the canvas, no toast");
    const all = await win.findAll({ name: /pass `ids`/ });
    assert.deepEqual(all, [], "no “(pass `ids`)” anywhere");
  }
});

sawitTest("canvas: a connector's context bar names its line and its ends, and their menus", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  await win.click({ role: "switch", name: "Connector", within: "Canvas tools" });
  await win.waitFor({ role: "switch", name: "Connector", within: "Canvas tools", value: true });
  // Drawn through the canvas's own pointer input (see canvas.test.js): no real pointer moves.
  const at = (kind, x, y) => ctx.call({ cmd: "canvas", op: { op: "pointer", kind, x, y } });
  await at("down", 120, 120);
  await at("move", 260, 200);
  const drawn = await at("up", 380, 260);
  assert.equal(drawn.selection?.length, 1, `the connector is selected: ${JSON.stringify(drawn)}`);

  // The context bar's buttons show icons only; each is named by what it sets and its value.
  const line = await win.waitFor({ role: "button", name: /^Line: \S/ });
  const start = await win.waitFor({ role: "button", name: /^Start: \S/ });
  const end = await win.waitFor({ role: "button", name: /^End: \S/ });
  assert.ok(line && start && end);
  // The start's menu is headed "Line start"; each option is an icon and its words, the words
  // read as text (the item itself, drawn from elements, has no name of its own).
  await win.openMenu({ role: "button", name: start.name });
  await win.waitFor({ role: "menuitem", name: "Line start" });
  const current = start.name.replace("Start: ", "");
  await win.waitFor({ role: "statictext", name: current });
  const items = await win.findAll({ role: "menuitem" });
  assert.ok(items.length >= 3, `the heading and the options: ${items.length} items`);
  await win.press("escape");
  await win.waitForGone({ role: "menuitem", name: "Line start" });
});
