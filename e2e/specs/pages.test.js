// The pages the footer's Pages menu (or the status bar's version) opens, each in its own tab, and
// the account menu at the title bar's end listing the same pages.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { activeTab, openPage, waitForTab } from "../lib/sawit.js";

/** Each page: its item in the Pages menu (or the button that opens it), its tab's kind and title,
 * and a control only it has. */
const PAGES = [
  { item: "Dev Tools", kind: "tools", tab: "Dev Tools", landmark: { role: "textfield", name: "Search tools" } },
  { item: "MCP Servers", kind: "mcp", tab: "MCP", landmark: { role: "textfield", name: "Search servers" } },
  { item: "Skills", kind: "skills", tab: "Skills", landmark: { role: "textfield", name: "Search skills" } },
  // Only opened: never press its Create Project, which really runs create-better-t-stack.
  {
    item: "New Project",
    kind: "new_project",
    tab: "New Project",
    landmark: { role: "textfield", name: "Filter Options" },
  },
  { item: "Canvas", kind: "canvas", tab: "Canvas", landmark: { name: "Canvas tools" } },
  {
    item: "Browser",
    kind: "browser",
    tab: "Browser",
    landmark: { role: "textfield", name: "Search or enter address" },
  },
  {
    item: "Keymap Editor",
    kind: "keymap",
    tab: "Keymap Editor",
    landmark: { role: "textfield", name: "Filter action names…" },
  },
  // The version in the status bar opens the changelog.
  { button: /^v\d+\.\d+\.\d+/, kind: "changelog", tab: "Changelog" },
];

sawitTest("pages: the Pages menu opens Tools, MCP, Skills, Canvas, Browser and Keymap, the version the Changelog", async (ctx) => {
  const { win } = ctx;
  for (const page of PAGES) {
    if (page.item) await openPage(ctx, page.item);
    else await win.click({ role: "button", name: page.button });
    await waitForTab(ctx, page.kind);
    const tab = await win.waitFor({ role: "radiobutton", name: page.tab, within: "Tabs" });
    assert.equal(tab.value, true, `the ${page.tab} tab is the selected one`);
    if (page.landmark) await win.waitFor(page.landmark);
  }
  // A page opens once: a second click goes back to its tab rather than adding another.
  await openPage(ctx, "Dev Tools");
  await waitForTab(ctx, "tools");
  const tabs = await win.findAll({ role: "radiobutton", within: "Tabs" });
  assert.equal(tabs.length, PAGES.length, `one tab per page: ${tabs.map((t) => t.name).join(", ")}`);
  assert.equal((await activeTab(ctx)).title, "Dev Tools");
});

sawitTest("pages: the account menu lists the same pages under the sign-in", async (ctx) => {
  const { win } = ctx;
  // Signed out, as every test starts: the title bar's account control is "Sign In".
  await win.click({ role: "button", name: "Sign In to GitHub" });
  await win.waitFor({ role: "menuitem", name: "Sign In with GitHub…" });
  for (const page of PAGES.filter((p) => p.item)) {
    await win.waitFor({ role: "menuitem", name: page.item });
  }
  await win.click({ role: "menuitem", name: "Skills" });
  await waitForTab(ctx, "skills");
  await win.waitFor({ role: "textfield", name: "Search skills" });
});
