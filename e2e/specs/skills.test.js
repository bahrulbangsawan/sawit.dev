// The Skills page: ten packs, Sawit's own Canvas Design first, and each install sheet's "Run with"
// package-manager menu. The sheet opens from a row's "Install …" button (the ellipsis: it opens a
// sheet that asks first); the sheet's own Install is never pressed.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { waitForTab } from "../lib/sawit.js";

/** What the menu may offer: the managers installed on this Mac, each with the command it runs. */
const MANAGERS = /^(npm \(npx\)|pnpm \(pnpm dlx\)|bun \(bunx\)|yarn \(yarn dlx\))$/;

sawitTest("skills: the page lists ten packs, and an install sheet picks its package manager", async (ctx) => {
  const { win } = ctx;
  await win.click({ role: "button", name: "Skills" });
  await waitForTab(ctx, "skills");
  await win.waitFor({ role: "textfield", name: "Search skills" });
  const all = await win.waitFor({ role: "switch", name: /^All \d+$/ });
  assert.equal(all.name, "All 10", "ten packs");
  // Eight a page: Cloudflare Skills, the last Platforms & SDKs pack, and Animate, the one Video &
  // Animation pack, are on the second.
  await win.waitFor({ role: "statictext", name: "1–8 of 10 packs" });
  // Each pack is a row named after it.
  for (const pack of ["Canvas Design", "Interface Skills", "Superpowers", "improve", "cf CLI"]) {
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
