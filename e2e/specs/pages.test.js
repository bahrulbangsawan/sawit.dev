// The pages a click in the title bar (or the status bar's version) opens, each in its own tab.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { activeTab, waitForTab } from "../lib/sawit.js";

/** Each page: the button that opens it, its tab's kind and title, and a control only it has. */
const PAGES = [
  { button: "Tools", kind: "tools", tab: "Dev Tools", landmark: { role: "textfield", name: "Search tools" } },
  { button: "MCP", kind: "mcp", tab: "MCP", landmark: { role: "textfield", name: "Search servers" } },
  { button: "Skills", kind: "skills", tab: "Skills", landmark: { role: "textfield", name: "Search skills" } },
  // Only opened: never press its Create Project, which really runs create-better-t-stack.
  {
    button: "New Project",
    kind: "new_project",
    tab: "New Project",
    landmark: { role: "textfield", name: "Filter Options" },
  },
  { button: "Canvas", kind: "canvas", tab: "Canvas", landmark: { name: "Canvas tools" } },
  {
    button: "Browser",
    kind: "browser",
    tab: "Browser",
    landmark: { role: "textfield", name: "Search or enter address" },
  },
  {
    button: "Keymap Editor",
    kind: "keymap",
    tab: "Keymap Editor",
    landmark: { role: "textfield", name: "Filter action names…" },
  },
  // The version in the status bar opens the changelog.
  { button: /^v\d+\.\d+\.\d+/, kind: "changelog", tab: "Changelog" },
];

sawitTest("pages: the title bar opens Tools, MCP, Skills, Canvas, Browser, Keymap and Changelog", async (ctx) => {
  const { win } = ctx;
  for (const page of PAGES) {
    await win.click({ role: "button", name: page.button });
    await waitForTab(ctx, page.kind);
    const tab = await win.waitFor({ role: "radiobutton", name: page.tab, within: "Tabs" });
    assert.equal(tab.value, true, `the ${page.tab} tab is the selected one`);
    if (page.landmark) await win.waitFor(page.landmark);
  }
  // A page opens once: a second click goes back to its tab rather than adding another.
  await win.click({ role: "button", name: "Tools" });
  await waitForTab(ctx, "tools");
  const tabs = await win.findAll({ role: "radiobutton", within: "Tabs" });
  assert.equal(tabs.length, PAGES.length, `one tab per page: ${tabs.map((t) => t.name).join(", ")}`);
  assert.equal((await activeTab(ctx)).title, "Dev Tools");
});
