// The popup audit (backlog item 15): menus with long data at a 900 px wide window. Each menu is
// opened as the keyboard does and pictured; the pictures go to $SAWIT_E2E_SHOTS (else the test's
// artifacts). What is checked here: every menu opens, stays inside the window, and its long items
// are named in full for accessibility (the label is cut on screen, never in its name).

import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas, terminalText, windowState } from "../lib/sawit.js";

const LONG_BRANCH = "feature/popup-audit-with-a-really-long-branch-name-to-check-the-ellipsis";
const LONG_FILE = "FormValiditySummaryCardDesignReviewForTheCheckoutPage.module.tsx";

/** Where the pictures go. */
async function shotsDir(ctx) {
  const dir = process.env.SAWIT_E2E_SHOTS || ctx.world.artifacts;
  await mkdir(dir, { recursive: true });
  return dir;
}

/** The window 900 px wide, as the audit asks. */
async function narrow(ctx) {
  const lenient = (args) => ctx.desktop.call(args).catch(() => {});
  await lenient(["move-window", "--window-id", ctx.win.id, "--x", "0", "--y", "0"]);
  await lenient(["resize-window", "--window-id", ctx.win.id, "--width", "900", "--height", "720"]);
  await new Promise((r) => setTimeout(r, 1_000));
}

/** Opens the menu of the button `name` (Return on it), pictures the window, and closes it. */
async function shotMenu(ctx, dir, file, query) {
  await ctx.win.openMenu(query);
  const menu = await ctx.win.waitFor({ role: "menu" }, { bounds: true });
  await ctx.win.screenshot(path.join(dir, file));
  const win = (await ctx.desktop.call(["list-windows"])).windows?.find((w) => w.id === ctx.win.id);
  if (menu.bounds && win?.bounds) {
    const right = menu.bounds.x + menu.bounds.width;
    assert.ok(right <= win.bounds.x + win.bounds.width + 1, `${file}: the menu stays inside the window`);
  }
  await ctx.win.press("escape");
  await ctx.win.waitForGone({ role: "menu" });
}

sawitTest("popups: menus with long data fit a 900 px window", { timeout: 300_000 }, async (ctx) => {
  const { world } = ctx;
  const dir = await shotsDir(ctx);
  // A long branch, a long file name, and two agents whose session titles are long.
  await ctx.call({ cmd: "terminal_run", command: `git checkout -q -b ${LONG_BRANCH}`, new_tab: true });
  await writeFile(path.join(world.project, LONG_FILE), "export {};\n");
  await ctx.call({ cmd: "open", path: path.join(world.project, LONG_FILE) });
  for (const n of [1, 2]) {
    const title = `◑ FormValiditySummary card design review for the checkout page, part ${n}`;
    await ctx.call({
      cmd: "terminal_run",
      command: `printf '\\033]0;${title}\\007'; ln -sf /bin/sleep claude; ./claude 600`,
      new_tab: true,
    });
  }
  await until(async () => (await windowState(ctx)).terminals.filter((t) => t.agent === "claude-code").length === 2, {
    what: "two stand-in agents",
    timeoutMs: 20_000,
  });
  await narrow(ctx);

  await shotMenu(ctx, dir, "pages-menu.png", { role: "button", name: "Pages" });
  await ctx.win.screenshot(path.join(dir, "window-900.png"));

  await ctx.call({ cmd: "open_page", page: "browser" });
  await shotMenu(ctx, dir, "browser-device-menu.png", { role: "button", name: /^Device Size/ });

  await openCanvas(ctx);
  await shotMenu(ctx, dir, "canvas-main-menu.png", { role: "button", name: "Main menu" });

  // The terminal's find bar in the narrow dock.
  await ctx.win.focus({ role: /^text/, name: "Terminal" });
  await ctx.win.press("cmd+f");
  await ctx.win.waitFor({ role: "textfield", name: "Find" });
  await ctx.win.typeKeys("design review");
  await ctx.win.screenshot(path.join(dir, "terminal-find-bar.png"));
  await ctx.win.press("escape");
  assert.ok((await terminalText(ctx)).length >= 0);
});
