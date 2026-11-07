// Agent Accounts (item 9): the Accounts page in a throwaway home. No real account folder, no
// sign-in, no Claude Code: the page lists the ~/.claude-* folders of the test's own home.

import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { openPage } from "../lib/sawit.js";
import { seedAccounts } from "../lib/accounts-seed.js";

sawitTest("accounts: the page opens from the Pages menu and offers to adopt a folder", async (ctx) => {
  const { win, world } = ctx;
  // A folder of a second account, made after launch: Adopt Existing lists it.
  await mkdir(path.join(world.home, ".claude-work"), { recursive: true });
  const state = await ctx.call({ cmd: "state" });
  assert.deepEqual(state.agent_accounts, [], "no accounts in a home without ~/.claude");

  await openPage(ctx, "Accounts");
  // No account yet: the empty state offers both ways in.
  await win.waitFor({ role: "button", name: "Add Account" });
  await win.waitFor({ role: "button", name: "Adopt Existing…" });
  await win.openMenu({ role: "button", name: "Adopt Existing…" });
  await win.click({ role: "menuitem", name: path.join(world.home, ".claude-work") });
  await win.waitFor({ role: "button", name: "Set Active" });
  const after = await ctx.call({ cmd: "state" });
  assert.equal(after.agent_accounts.length, 1);
  assert.equal(after.agent_accounts[0].name, "work");
});

sawitTest("accounts: an agent opens the page without taking the keyboard", async (ctx) => {
  const { win } = ctx;
  await ctx.call({ cmd: "open_page", page: "accounts" });
  await win.waitFor({ role: "tablist", name: "Account Sections" });
});

sawitTest("accounts: the Settings tab offers the shell block, the shortcuts and Open at Login", async (ctx) => {
  const { win } = ctx;
  await ctx.call({ cmd: "open_page", page: "accounts" });
  await win.click({ role: "radiobutton", name: "Settings", within: "Account Sections" });
  // The test's own ~/.zshrc has no block yet: only Install is offered, nothing is written.
  await win.waitFor({ role: "button", name: "Install…" });
  await win.waitFor({ role: "button", name: "Install Shortcuts" });
  await win.waitFor({ name: "Share Between Accounts" });
});

sawitTest("accounts: the Status Line tab previews the layout and offers presets", async (ctx) => {
  const { win } = ctx;
  await ctx.call({ cmd: "open_page", page: "accounts" });
  await win.click({ role: "radiobutton", name: "Status Line", within: "Account Sections" });
  await win.waitFor({ role: "button", name: "Apply" });
  await win.waitFor({ role: "button", name: "Start From…" });
  await win.waitFor({ role: "button", name: "Undo Last Apply" });
});

sawitTest("accounts: the Usage and Diagnostics tabs open", async (ctx) => {
  const { win } = ctx;
  await ctx.call({ cmd: "open_page", page: "accounts" });
  await win.click({ role: "radiobutton", name: "Usage", within: "Account Sections" });
  await win.waitFor({ role: "switch", name: "7 Days", value: true });
  await win.click({ role: "radiobutton", name: "Diagnostics", within: "Account Sections" });
  // The test's PATH has no claude: Diagnostics says so, with the fix.
  await win.waitFor({ name: /Install Claude Code/ });
});

sawitTest("accounts: no Claude Pool in the home, no import or removal offered", async (ctx) => {
  const { win } = ctx;
  await ctx.call({ cmd: "open_page", page: "accounts" });
  await win.waitFor({ role: "button", name: "Add Account" });
  assert.equal((await win.findAll({ role: "button", name: "Import from Claude Pool…" })).length, 0);
  await win.click({ role: "radiobutton", name: "Settings", within: "Account Sections" });
  await win.waitFor({ role: "button", name: "Install Shortcuts" });
  assert.equal((await win.findAll({ role: "button", name: "Remove Claude Pool…" })).length, 0);
});

// B14: the redesigned page. The tabs are a real tab list, each account a named list item with
// its cp badge (never "1 1"), ↑ and ↓ select in the list, and the alert threshold is disabled
// while alerts are off.
sawitTest("accounts: tabs, named rows, keyboard selection and the threshold", async (ctx) => {
  const { win, world } = ctx;
  for (const name of ["work", "side"]) {
    await mkdir(path.join(world.home, `.claude-${name}`), { recursive: true });
  }
  await ctx.call({ cmd: "open_page", page: "accounts" });
  await win.waitFor({ role: "tablist", name: "Account Sections" });
  for (const tab of ["Accounts", "Usage", "Status Line", "Diagnostics", "Settings"]) {
    await win.waitFor({ role: "radiobutton", name: tab, within: "Account Sections" });
  }
  for (const [ix, name] of ["work", "side"].entries()) {
    // The empty state offers Adopt Existing…; once a row exists, it is in Add Account's panel.
    if (ix > 0) await win.click({ role: "button", name: "Add Account" });
    await win.openMenu({ role: "button", name: "Adopt Existing…" });
    await win.click({ role: "menuitem", name: path.join(world.home, `.claude-${name}`) });
    await win.waitFor({ name: new RegExp(`^${name}, cp\\d`) });
  }
  // Each row says its name once, with its cp number.
  await win.waitFor({ name: /^work, cp1/ });
  await win.waitFor({ name: /^side, cp2/ });
  // The first row is selected; ↓ in the list selects the next.
  await win.waitFor({ role: "button", name: "More for work" });
  await win.focus({ name: "Claude Code Accounts" });
  await win.press("down");
  await win.waitFor({ role: "button", name: "More for side" });
  await win.press("up");
  await win.waitFor({ role: "button", name: "More for work" });

  // Settings: alerts are off by default, so the threshold can't be changed yet.
  await win.click({ role: "radiobutton", name: "Settings", within: "Account Sections" });
  const threshold = await win.waitFor({ name: "Alert at 85%" });
  assert.ok(
    threshold.states.some((s) => /disabled|unavailable/i.test(s)) || threshold.states.includes("!enabled"),
    `the threshold is disabled while alerts are off: ${JSON.stringify(threshold.states)}`,
  );
  await win.click({ role: "switch", name: "Notify Near and at a Limit" });
  await win.waitFor({ role: "switch", name: "Notify Near and at a Limit", value: true });
});

sawitTest(
  "accounts: design screenshots of every tab, wide and narrow",
  { args: (world) => { seedAccounts(world); return undefined; } },
  async (ctx) => {
    const { win, world } = ctx;
    await ctx.call({ cmd: "open_page", page: "accounts" });
    await win.waitFor({ name: /^work, cp1/ });
    const shoot = async (name) => {
      await new Promise((r) => setTimeout(r, 600));
      // SAWIT_E2E_SHOTS keeps them past a passing run (a passing test's artifacts are removed).
      const dir = process.env.SAWIT_E2E_SHOTS || world.artifacts;
      mkdirSync(dir, { recursive: true });
      await win.screenshot(path.join(dir, `${name}.png`));
    };
    for (const [w, h, size] of [[1440, 900, "wide"], [720, 480, "narrow"]]) {
      await win.resize(w, h);
      for (const tab of ["Accounts", "Usage", "Status Line", "Diagnostics", "Settings"]) {
        await win.click({ role: "radiobutton", name: tab, within: "Account Sections" });
        await shoot(`${size}-${tab.toLowerCase().replace(" ", "-")}`);
      }
    }
    // The seeded state reads right: cp numbers, the at-limit account and "% used".
    await win.click({ role: "radiobutton", name: "Accounts", within: "Account Sections" });
    await win.waitFor({ name: /^client, cp3, At limit/ });
    await win.waitFor({ name: /^work, cp1, Active/ });
    await win.waitFor({ name: /^5-hour window: 23% used/ });
  },
);
