// The terminal tab menu as complete as the file tab menu (backlog item 22): Safe Close All (an
// agent's scope idle) keeps the busy tab, Force Close All (scope all) needs confirm while
// something runs, and the menu shows every item.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { terminalText, windowState } from "../lib/sawit.js";

async function terminals(ctx) {
  return (await windowState(ctx)).terminals;
}

async function newTerminal(ctx) {
  const before = (await terminals(ctx)).length;
  await ctx.win.click({ role: "button", name: "New Terminal" });
  await until(async () => (await terminals(ctx)).length === before + 1, { what: "a new terminal tab" });
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), {
    timeoutMs: 20_000,
    what: "the new shell's prompt",
  });
}

sawitTest("terminal menus: a safe close keeps the busy tab, a forced one needs confirm", async (ctx) => {
  const { win } = ctx;
  await newTerminal(ctx);
  await newTerminal(ctx);
  await win.typeKeys("sleep 600");
  await win.press("return");
  await until(async () => (await terminals(ctx)).some((t) => t.active && t.running_job), {
    what: "sleep to run",
  });
  const busy = (await terminals(ctx)).findIndex((t) => t.active);

  // An agent's scoped closes: all is refused while sleep runs, idle closes the others.
  await assert.rejects(ctx.call({ cmd: "terminal_close", tab: busy, scope: "all" }), /sleep/);
  const idle = await ctx.call({ cmd: "terminal_close", tab: busy, scope: "idle" });
  assert.ok(idle.closed >= 1, JSON.stringify(idle));
  await until(async () => (await terminals(ctx)).length === 1, { what: "only the busy tab to stay" });
  assert.ok((await terminals(ctx))[0].running_job);
  // Nothing is left of the first tab: Close Left closes nothing.
  const left = await ctx.call({ cmd: "terminal_close", tab: 0, scope: "left" });
  assert.equal(left.closed, 0);
  assert.equal((await terminals(ctx)).length, 1);
});

sawitTest(
  "terminal menus: right-click a tab, and the menu has the file tab menu's items",
  { headed: true },
  async (ctx) => {
    const { win } = ctx;
    await newTerminal(ctx);
    await win.rightClick({ role: "radiobutton", within: "Terminals", value: true });
    await win.waitFor({ role: "menuitem", name: "Rename Tab" });
    const names = (await win.findAll({ role: "menuitem" })).map((m) => m.name);
    for (const label of [
      "Rename Tab",
      "Duplicate Tab",
      "Move to Project",
      "Remove from Group",
      "Close",
      "Close Others",
      "Close Left",
      "Close Right",
      "Safe Close All",
      "Force Close All",
      "Lock Tab",
      "Pin Tab",
      "Copy Last Output",
      "Copy Error",
      "Clear",
      "Export Output…",
      "Full Screen",
    ]) {
      assert.ok(names.includes(label), `${label} in ${names.join(", ")}`);
    }
    await win.press("escape");
    await win.waitForGone({ role: "menuitem", name: "Rename Tab" });
  },
);
