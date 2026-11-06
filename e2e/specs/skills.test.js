// The Skills page: Sawit Skills' card first, fourteen packs (Sawit's own Canvas Design first, its
// Sawit pack first of the Workflow ones), and each install sheet's "Run with" package-manager menu.
// The sheet opens from a row's "Install …" button (the ellipsis: it opens a sheet that asks
// first); the sheet's own Install is never pressed, nor the card's.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { openPage, waitForTab } from "../lib/sawit.js";

/** What the menu may offer: the managers installed on this Mac, each with the command it runs. */
const MANAGERS = /^(npm \(npx\)|pnpm \(pnpm dlx\)|bun \(bunx\)|yarn \(yarn dlx\))$/;

sawitTest("skills: the page lists Sawit Skills and fourteen packs, and an install sheet picks its package manager", async (ctx) => {
  const { win } = ctx;
  // The footer's Pages menu opens it (there is no Skills button of its own any more).
  await openPage(ctx, "Skills");
  await waitForTab(ctx, "skills");
  await win.waitFor({ role: "textfield", name: "Search skills" });
  // Sawit Skills' card comes first, with the footer's actions: a fresh home has none of it.
  await win.waitFor({ role: "group", name: "Sawit Skills" });
  await win.waitFor({ role: "button", name: "Install Sawit Skills", within: "Sawit Skills" });
  const all = await win.waitFor({ role: "switch", name: /^All \d+$/ });
  assert.equal(all.name, "All 14", "fourteen packs");
  // Eight a page: the Platforms & SDKs and Video & Animation packs are on the second.
  await win.waitFor({ role: "statictext", name: "1–8 of 14 packs" });
  // Each pack is a row named after it.
  // The first page: the six Design & UI packs, then Sawit and Superpowers.
  for (const pack of ["Canvas Design", "Interface Skills", "Design to Code", "shadcn/ui", "Sawit", "Superpowers"]) {
    await win.waitFor({ role: "group", name: pack });
  }

  // Interface Skills installs through the skills CLI, which runs with a package manager.
  await win.click({ role: "button", name: "Install Interface Skills…" });
  const sheet = await win.waitFor({ role: "dialog", name: "Interface Skills" });
  const menu = await win.waitFor({ role: "button", name: /^Run with \w+$/, within: sheet.name });
  await win.openMenu({ role: "button", name: menu.name, within: sheet.name });
  await win.waitFor({ role: "menuitem", name: MANAGERS });
  const items = (await win.findAll({ role: "menuitem" })).filter((i) => i.name);
  for (const item of items) assert.match(item.name, MANAGERS, "only npm, pnpm, bun or yarn");
  assert.ok(items.length >= 1 && items.length <= 4, items.map((i) => i.name).join(", "));

  // Picking another manager, when this Mac has another, renames the button after it.
  const current = menu.name.replace("Run with ", "");
  const other = items.find((i) => i.name.split(" ")[0] !== current);
  if (other) {
    await win.click({ role: "menuitem", name: other.name });
    await win.waitFor({ role: "button", name: `Run with ${other.name.split(" ")[0]}`, within: sheet.name });
  } else {
    await win.press("escape");
  }
  await win.waitForGone({ role: "menuitem", name: MANAGERS });
  await win.click({ role: "button", name: "Close", within: sheet.name });
  await win.waitForGone({ role: "dialog", name: sheet.name });
});
