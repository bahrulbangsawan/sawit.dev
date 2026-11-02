// Starting Sawit: with a project it opens a project window, with nothing the welcome screen.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { windowState } from "../lib/sawit.js";

sawitTest("launch: a project folder opens a project window", async (ctx) => {
  const { win, world } = ctx;
  const state = await windowState(ctx);
  assert.equal(state.welcome_screen, false);
  assert.equal(state.project, world.project);

  // The project is the title bar's active project, and its files are in the sidebar.
  const chip = await win.waitFor({ role: "radiobutton", name: "fixture-app", within: "Projects" });
  assert.equal(chip.value, true);
  // The pages are in the footer's Pages menu (and under the account at the title bar's end).
  await win.waitFor({ role: "button", name: "Pages" });
  await win.waitFor({ role: "button", name: "Sign In to GitHub" });
  await win.waitFor({ name: "README.md", within: "Files" });
  await win.waitFor({ name: "src", within: "Files" });
});

sawitTest(
  "launch: no folder opens the welcome screen",
  { args: () => [] },
  async (ctx) => {
    const { win } = ctx;
    const state = await windowState(ctx);
    assert.equal(state.welcome_screen, true);
    assert.equal(state.project, null);
    await win.click({ role: "button", name: "Continue Without a Project" });
    await win.waitForGone({ role: "button", name: "Continue Without a Project" });
    assert.equal((await windowState(ctx)).welcome_screen, false);
  },
);
