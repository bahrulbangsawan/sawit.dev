// The file editor: a file opened from the tree takes typing, and ⌘S writes it to disk.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { activeTab } from "../lib/sawit.js";

sawitTest("editor: open a file from the tree, type, save, and the file on disk has it", async (ctx) => {
  const { win, world } = ctx;
  const file = path.join(world.project, "notes.txt");
  await win.click({ role: "treeitem", name: "notes.txt", within: "Files" });
  await until(async () => (await activeTab(ctx))?.path === file, { what: "notes.txt in a tab" });
  await win.waitFor({ role: "radiobutton", name: "notes.txt", within: "Tabs", value: true });

  // The editor is a text field named after its file, holding the file's text.
  const editor = { role: "textfield", name: "notes.txt" };
  assert.equal((await win.waitFor(editor)).value, "first line\nsecond line\n");
  // The keyboard goes to it, at the end of the text, and the keys typed land there.
  await win.focus(editor);
  await win.press("cmd+down");
  await win.typeKeys("third line");
  await win.waitFor({ ...editor, value: "first line\nsecond line\nthird line" });
  await until(async () => (await activeTab(ctx))?.unsaved === true, { what: "the tab to be unsaved" });

  await win.press("cmd+s");
  const saved = await until(
    async () => {
      const text = await readFile(file, "utf8");
      return text.includes("third line") ? text : null;
    },
    { what: "notes.txt on disk to have the typed line" },
  );
  assert.equal(saved, "first line\nsecond line\nthird line");
  await until(async () => (await activeTab(ctx))?.unsaved === false, { what: "the tab to be saved" });
});
