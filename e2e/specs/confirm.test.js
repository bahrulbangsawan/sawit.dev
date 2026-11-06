// The shared confirm question (backlog item 13), through a real destructive flow: closing a
// terminal whose shell runs a command. The question names what would stop, Cancel has the
// keyboard (Return cancels and the terminal stays), and its red "Close and Stop" closes it.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { terminalText, windowState } from "../lib/sawit.js";

/** The terminal tabs, as `app_state` lists them. */
async function terminals(ctx) {
  return (await windowState(ctx)).terminals;
}

const GO = { role: "button", name: "Close and Stop" };

sawitTest("confirm: closing a terminal that runs something asks first, and Return cancels", async (ctx) => {
  const { win } = ctx;
  const before = (await terminals(ctx)).length;
  await win.click({ role: "button", name: "New Terminal" });
  await until(async () => (await terminals(ctx)).length === before + 1, { what: "a new terminal tab" });
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), {
    timeoutMs: 20_000,
    what: "the new shell's prompt",
  });
  await win.typeKeys("sleep 600");
  await win.press("return");
  await until(async () => (await terminals(ctx)).some((t) => t.active && t.running_job), {
    what: "app_state to say the tab runs something",
  });

  // ⌥⌘W (Close Pane) on the busy terminal: the question, naming sleep.
  await win.press("alt+cmd+w");
  await win.waitFor(GO);
  const cancel = await win.waitFor({ role: "button", name: "Cancel" });
  assert.ok(cancel, "the question has a Cancel button");
  // Return answers Cancel: the question goes, and the terminal stays.
  await win.press("return");
  await win.waitForGone(GO);
  assert.equal((await terminals(ctx)).length, before + 1, "Return closed the terminal");

  // Escape cancels too.
  await win.press("alt+cmd+w");
  await win.waitFor(GO);
  await win.press("escape");
  await win.waitForGone(GO);
  assert.equal((await terminals(ctx)).length, before + 1, "Escape closed the terminal");

  // The red button closes it, and stops sleep.
  await win.press("alt+cmd+w");
  await win.click(GO);
  await until(async () => (await terminals(ctx)).length === before, { what: "the terminal to close" });
});
