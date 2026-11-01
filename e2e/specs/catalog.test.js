// The Tools and MCP pages' lists: search, the filter tabs, and the rows-per-page picker. Nothing
// here installs, updates or removes anything: desktop.js refuses those buttons outright.
//
// How the lists read through accessibility: each row is a group named after its tool or server,
// the filter tabs are switches ("Installed 9", on or off), and the count under the list is text
// ("1–8 of 24 tools").

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { waitForTab } from "../lib/sawit.js";

const COUNT = /^(\d+)–(\d+) of (\d+) \w+$/;

/** The count under the list, as numbers: `{ first, last, total, name }`. */
async function count(win, query = {}) {
  const node = await win.waitFor({ role: "statictext", name: COUNT, ...query });
  const [, first, last, total] = node.name.match(COUNT).map(Number);
  return { first, last, total, name: node.name };
}

/** The total a filter tab names: "Installed 8" → 8. */
const tabTotal = (node) => Number(node.name.match(/(\d+)$/)[1]);

const PAGES = [
  {
    button: "Tools",
    kind: "tools",
    search: "Search tools",
    query: "claude",
    hit: "Claude Code",
    miss: "Homebrew",
  },
  {
    button: "MCP",
    kind: "mcp",
    search: "Search servers",
    query: "figma",
    hit: "Figma",
    miss: "Neon",
  },
];

for (const page of PAGES) {
  sawitTest(`${page.button}: search narrows the list, and clearing it brings every row back`, async (ctx) => {
    const { win } = ctx;
    await win.click({ role: "button", name: page.button });
    await waitForTab(ctx, page.kind);
    const all = await win.waitFor({ role: "switch", name: /^All \d+$/ });
    const before = await count(win);
    assert.equal(before.total, tabTotal(all), `${before.name} counts every row`);
    await win.waitFor({ role: "group", name: page.miss });

    await win.setValue({ role: "textfield", name: page.search }, page.query);
    await win.waitFor({ role: "group", name: page.hit });
    await win.waitForGone({ role: "group", name: page.miss });
    const narrowed = await count(win);
    assert.ok(narrowed.total < before.total, `${narrowed.name} is fewer than ${before.total}`);

    await win.setValue({ role: "textfield", name: page.search }, "");
    await win.waitFor({ role: "statictext", name: before.name });
    await win.waitFor({ role: "group", name: page.miss });
  });

  sawitTest(`${page.button}: a filter tab shows its rows only`, async (ctx) => {
    const { win } = ctx;
    await win.click({ role: "button", name: page.button });
    await waitForTab(ctx, page.kind);
    // "Installed N": on a fresh home, what Homebrew put in /opt/homebrew (for Tools), or none.
    const installed = await win.waitFor({ role: "switch", name: /^Installed \d+$/ });
    const n = tabTotal(installed);
    await win.click({ role: "switch", name: installed.name });
    await win.waitFor({ role: "switch", name: installed.name, value: true });
    assert.equal((await win.waitFor({ role: "switch", name: /^All \d+$/ })).value, false);
    if (n === 0) {
      await win.waitFor({ role: "button", name: "Clear Filters" });
    } else {
      await win.waitFor({ role: "statictext", name: new RegExp(` of ${n} \\w+$`) });
    }
    await win.click({ role: "switch", name: /^All \d+$/ });
    await win.waitFor({ role: "switch", name: /^All \d+$/, value: true });
  });

  sawitTest(`${page.button}: the rows-per-page picker shows 16 rows a page`, async (ctx) => {
    const { win } = ctx;
    await win.click({ role: "button", name: page.button });
    await waitForTab(ctx, page.kind);
    const before = await count(win);
    assert.equal(before.last, Math.min(8, before.total), `8 rows a page at first: ${before.name}`);
    await win.openMenu({ role: "button", name: "Show 8" });
    await win.waitFor({ role: "menuitem", name: "Rows per page" });
    await win.click({ role: "menuitem", name: "16" });
    await win.waitFor({ role: "button", name: "Show 16" });
    const rows = Math.min(16, before.total);
    const after = await count(win, { name: new RegExp(`^1–${rows} of ${before.total} \\w+$`) });
    assert.equal(after.last, rows);
  });
}
