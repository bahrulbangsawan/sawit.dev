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
