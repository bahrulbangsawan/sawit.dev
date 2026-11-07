// C1: the account pool's views on the Accounts page, with three seeded Claude Code accounts
// (sample data: made-up names and figures). The pick, the urgency order, the timeline, the
// analytics, and a price set in the wallet.

import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import path from "node:path";

import { seedAccounts } from "../lib/accounts-seed.js";
import { sawitTest } from "../lib/harness.js";

const tab = (name) => ({ role: "radiobutton", name, within: "Account Sections" });

sawitTest(
  "account pool: every view opens, the pick has the most room, and a price saves",
  { args: (world) => { seedAccounts(world); return undefined; } },
  async (ctx) => {
    const { win, world } = ctx;
    await ctx.call({ cmd: "open_page", page: "accounts" });
    for (const name of ["Overview", "Detailed", "Compare", "Timeline", "Analytics", "Wallet"]) {
      await win.waitFor(tab(name));
    }
    // Overview: work has 59% of its weekly limit left, more than personal (12%) and client (0%).
    await win.click(tab("Overview"));
    await win.waitFor({ name: /^Most room right now: work\. 59% of its weekly limit left/ });
    await win.waitFor({ role: "group", name: "client, Limit reached" });
    await win.waitFor({ role: "group", name: "personal, Running low" });

    // Detailed, by urgency: the account at its limit comes first.
    await win.click(tab("Detailed"));
    await win.click({ role: "switch", name: "By Urgency" });
    const rows = (await win.findAll({ role: "group", name: /, (Limit reached|Running low|Plenty left)$/ })).map((n) => n.name);
    assert.deepEqual(rows, ["client, Limit reached", "personal, Running low", "work, Plenty left"]);

    await win.click(tab("Compare"));
    await win.waitFor({ role: "button", name: "Open in Terminal" });
    await win.click(tab("Timeline"));
    await win.waitFor({ name: /^client: Limit reached/ });
    await win.click(tab("Analytics"));
    await win.waitFor({ name: /^Use by weekday and hour/ });

    // Wallet: a monthly price for work counts in the total.
    await win.click(tab("Wallet"));
    await win.click({ role: "button", name: "Edit Price" });
    await win.setValue({ name: "Amount" }, "200");
    await win.click({ role: "button", name: "Save" });
    await win.waitFor({ name: /^Per month: \$200\./ });

    // Screenshots for the design review, when asked for.
    const shots = process.env.SAWIT_E2E_SHOTS;
    if (shots) {
      mkdirSync(shots, { recursive: true });
      for (const [w, h, size] of [[1440, 900, "wide"], [720, 480, "narrow"]]) {
        await win.resize(w, h);
        for (const name of ["Overview", "Detailed", "Compare", "Timeline", "Analytics", "Wallet"]) {
          await win.click(tab(name));
          await new Promise((r) => setTimeout(r, 700));
          await win.screenshot(path.join(shots, `pool-${size}-${name.toLowerCase()}.png`));
        }
      }
    }
    const wallet = path.join(world.sawitHome, "accounts", "wallet.json");
    assert.ok((await import("node:fs")).existsSync(wallet), "the wallet is saved");
  },
);
