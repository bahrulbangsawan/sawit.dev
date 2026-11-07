// The keymap editor lists the commands, and its filter narrows the list.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { openPage, waitForTab } from "../lib/sawit.js";

sawitTest("keymap: the editor lists the commands, and the filter narrows them", async (ctx) => {
  const { win } = ctx;
  await openPage(ctx, "Keymap Editor");
  await waitForTab(ctx, "keymap");
  // A row per command, named "area: command" ("canvas: align left").
  await win.waitFor({ role: "row", within: "Keybindings" });
  const rows = await win.findAll({ role: "row", within: "Keybindings" });
  assert.ok(rows.length >= 10, `a screenful of commands (${rows.length})`);
  for (const row of rows) assert.match(row.name ?? "", /^[a-z -]+: \S/i, "a row is named by its command");

  await win.setValue({ role: "textfield", name: "Filter action names…" }, "new terminal");
  await win.waitFor({ role: "row", within: "Keybindings", name: /new terminal/i });
  // The filter matches every word in a command's name, keys or context: fewer rows show.
  const filtered = await win.findAll({ role: "row", within: "Keybindings" });
  assert.ok(filtered.length < rows.length, `${filtered.length} rows, from ${rows.length}`);
  assert.ok(!filtered.some((r) => /^canvas: align/.test(r.name)), "rows that don't match are gone");
});

// B5: one key opens and closes each area, and with ⇧ fills the window with it.
import { until } from "../lib/wait.js";
import { activeTab, windowState } from "../lib/sawit.js";

async function layoutIs(ctx, what, test) {
  return until(async () => {
    const w = await windowState(ctx);
    return test(w) ? w : null;
  }, { what });
}

sawitTest("keymap: the area keys open, close and fill each area", { timeout: 180_000 }, async (ctx) => {
  const { win } = ctx;
  await win.focus({ role: /^text/, name: "Terminal" });
  // ⌘E: the folder panel hides, and shows again.
  await win.press("cmd+e");
  await layoutIs(ctx, "the folder panel hidden", (w) => w.layout?.sidebar === false);
  await win.press("cmd+e");
  await layoutIs(ctx, "the folder panel back", (w) => w.layout?.sidebar === true);
  // ⌘U: the canvas opens in front; ⌘U again hides it.
  await win.press("cmd+u");
  await until(async () => (await activeTab(ctx))?.kind === "canvas", { what: "the canvas in front" });
  await win.press("cmd+u");
  await until(async () => (await activeTab(ctx))?.kind !== "canvas" || !(await windowState(ctx)).layout?.center, {
    what: "the canvas hidden",
  });
  // ⌘B: the browser opens in front.
  await win.press("cmd+b");
  await until(async () => (await activeTab(ctx))?.kind === "browser", { what: "the browser in front" });
  // ⇧⌘B fills the window with it, and puts the layout back.
  await win.press("cmd+shift+b");
  await layoutIs(ctx, "the browser filling the window", (w) => w.maximized === "browser");
  await win.press("cmd+shift+b");
  await layoutIs(ctx, "the layout back", (w) => !w.maximized);
  // ⌘T hides the terminal and shows it again, from the browser too (its New Tab is ⌃T).
  await win.press("cmd+t");
  await layoutIs(ctx, "the terminal hidden", (w) => w.layout?.terminal === false);
  await win.press("cmd+t");
  await layoutIs(ctx, "the terminal back", (w) => w.layout?.terminal === true);
  // ⌘⇧N opens New Project.
  await win.press("cmd+shift+n");
  await until(async () => (await activeTab(ctx))?.kind === "new_project", { what: "the New Project page" });
  assert.ok(true);
});
