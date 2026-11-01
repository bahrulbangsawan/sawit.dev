// Small readers of Sawit's own state, over its control socket, for tests to assert on.

import { until } from "./wait.js";

/** The window's state, as `app_state` shows it (the first window: each test has one). */
export async function windowState(ctx) {
  const state = await ctx.call({ cmd: "state" });
  return state.windows[0];
}

/** The active center tab: `{ index, title, kind, path, ... }`, or undefined. */
export async function activeTab(ctx) {
  return (await windowState(ctx)).tabs.find((t) => t.active);
}

/** Waits until the active center tab is of `kind` (`tools`, `mcp`, `canvas`, …). */
export async function waitForTab(ctx, kind, { timeoutMs = 15_000 } = {}) {
  return until(
    async () => {
      const tab = await activeTab(ctx);
      return tab?.kind === kind ? tab : null;
    },
    { timeoutMs, what: `the active tab to be a ${kind} tab` },
  );
}

/** Opens the canvas from the title bar and waits for its toolbar. */
export async function openCanvas(ctx) {
  await ctx.win.click({ role: "button", name: "Canvas" });
  await waitForTab(ctx, "canvas");
  return ctx.win.waitFor({ role: "toolbar", name: "Canvas tools" });
}

/**
 * The text of a terminal pane, through `terminal_read`. `tab` is the terminal tab's index, `pane`
 * its pane's: without them, the pane the user has in front.
 */
export async function terminalText(ctx, { tab, pane, lines = 50 } = {}) {
  const { text } = await ctx.call({ cmd: "terminal_read", tab, pane, lines });
  return text;
}

/** Waits until a line of the terminal pane is exactly `line` (output, not the typed command). */
export async function waitForTerminalLine(ctx, line, opts = {}) {
  return until(
    async () => {
      const text = await terminalText(ctx, opts);
      return text.split("\n").some((l) => l.trimEnd() === line) ? text : null;
    },
    { timeoutMs: opts.timeoutMs ?? 15_000, what: `the terminal line “${line}”` },
  );
}
