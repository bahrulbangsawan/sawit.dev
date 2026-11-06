// GitHub in a run with no sign-in (the test's HOME has no login keychain, so no sign-in can be
// found): the title bar offers the one sign-in, and the Config Files panel's footer shows only
// Sign In to GitHub (item 20), never Save Shell Files to GitHub beside it.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";

sawitTest("github: signed out, the shell files footer offers only the sign-in", async (ctx) => {
  const { win } = ctx;
  const state = await ctx.call({ cmd: "state" });
  assert.equal(state.github.signed_in, false);
  assert.equal(state.github.state, "signed_out");
  await win.waitFor({ role: "button", name: "Sign In to GitHub…" });

  await ctx.call({ cmd: "show_panel", panel: "shell_config" });
  await win.waitFor({ role: "button", name: "Sign In to GitHub" });
  const save = await win.findAll({ role: "button", name: /Save Shell Files to GitHub/ });
  assert.equal(save.length, 0, "Save Shell Files to GitHub shows while signed out");
});
