// Locking and pinning terminal tabs (backlog item 2): a locked tab shows a lock where its × was,
// an agent's close is refused even with confirm, ⌥⌘W doesn't close it; a pinned tab moves to the
// strip's start.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { terminalText, windowState } from "../lib/sawit.js";

async function terminals(ctx) {
  return (await windowState(ctx)).terminals;
}

sawitTest("terminal lock: a locked tab shows a lock and nothing closes it until unlocked", async (ctx) => {
  const { win } = ctx;
  const before = (await terminals(ctx)).length;
  await win.click({ role: "button", name: "New Terminal" });
  await until(async () => (await terminals(ctx)).length === before + 1, { what: "a new terminal tab" });
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), {
    timeoutMs: 20_000,
    what: "the new shell's prompt",
  });
  const tab = (await terminals(ctx)).findIndex((t) => t.active);

  const locked = await ctx.call({ cmd: "terminal_lock", tab, locked: true });
  assert.equal(locked.locked, true);
  await win.waitFor({ name: "Locked", within: "Terminals" });
  assert.ok((await terminals(ctx))[tab].locked, "app_state says it is locked");

  // An agent can't close it, even sure.
  await assert.rejects(ctx.call({ cmd: "terminal_close", tab, confirm: true }), /locked/);
  // Nor can ⌥⌘W (the panel says why instead).
  await win.press("alt+cmd+w");
  await new Promise((r) => setTimeout(r, 1500));
  assert.equal((await terminals(ctx)).length, before + 1);

  // Pinned, it moves to the start of the strip.
  const pinned = await ctx.call({ cmd: "terminal_pin", tab, pinned: true });
  assert.deepEqual(pinned, { terminal: 0, pinned: true });
  assert.ok((await terminals(ctx))[0].pinned);

  // Unlocked, it closes.
  await ctx.call({ cmd: "terminal_lock", tab: 0, locked: false });
  await win.waitForGone({ name: "Locked", within: "Terminals" });
  await ctx.call({ cmd: "terminal_close", tab: 0, confirm: true });
  await until(async () => (await terminals(ctx)).length === before, { what: "the unlocked tab to close" });
});
