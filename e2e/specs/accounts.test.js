// Agent Accounts (item 9): the Accounts page in a throwaway home. No real account folder, no
// sign-in, no Claude Code: the page lists the ~/.claude-* folders of the test's own home.

import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { openPage } from "../lib/sawit.js";

sawitTest("accounts: the page opens from the Pages menu and offers to adopt a folder", async (ctx) => {
  const { win, world } = ctx;
  // A folder of a second account, made after launch: Adopt Existing lists it.
  await mkdir(path.join(world.home, ".claude-work"), { recursive: true });
  const state = await ctx.call({ cmd: "state" });
  assert.deepEqual(state.agent_accounts, [], "no accounts in a home without ~/.claude");

  await openPage(ctx, "Accounts");
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
  await win.waitFor({ role: "button", name: "Adopt Existing…" });
});
