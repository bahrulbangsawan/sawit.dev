// New Project's Upgrade: with 3.49.0 pinned and a newer create-better-t-stack on npm, the page
// says so beside the version in use; Upgrade reads the release's options, switches to it, says
// what is new and gone and hides the gone option; Go Back returns to the pinned one.
//
// npm and the CLI are stood in for by Sawit's test hooks: SAWIT_STACK_CLI_LATEST is taken as npm's
// latest, and SAWIT_STACK_CLI_HELP's file as the release's `create --help` (3.49.0's, with
// solid-start added and mongoose gone). Create Project is never pressed.

import assert from "node:assert/strict";
import path from "node:path";

import { REPO } from "../lib/app.js";
import { sawitTest } from "../lib/harness.js";
import { openPage, waitForTab } from "../lib/sawit.js";

const env = () => ({
  SAWIT_STACK_CLI_LATEST: "3.50.0",
  SAWIT_STACK_CLI_HELP: path.join(REPO, "e2e", "fixtures", "help-newer.txt"),
});

sawitTest("new project: Upgrade switches to the newer CLI, hides what it dropped, and Go Back returns", { env }, async (ctx) => {
  const { win } = ctx;
  await openPage(ctx, "New Project");
  await waitForTab(ctx, "new_project");
  await win.waitFor({ role: "statictext", name: "Better-T-Stack v3.49.0" });
  await win.waitFor({ role: "radiobutton", name: /Mongoose/ }).catch(() => win.waitFor({ name: /Mongoose/ }));
  await win.screenshot(path.join(ctx.world.artifacts, "upgrade-offered.png"));

  // SAWIT_STACK_CLI_HELP answers for the CLI: the press runs nothing and downloads nothing.
  await win.click({ role: "button", name: "Upgrade to v3.50.0" }, { stoodIn: true });
  await win.waitFor({ role: "statictext", name: "Using Better-T-Stack v3.50.0" }, { timeoutMs: 30_000 });
  const diff = await win.waitFor({ role: "statictext", name: /Gone: --orm mongoose\./ });
  assert.match(diff.name, /^New: --frontend solid-start\. Gone/);
  await win.waitForGone({ name: /Mongoose/ });
  await win.waitFor({ role: "button", name: "Go Back to v3.49.0" });
  await win.screenshot(path.join(ctx.world.artifacts, "upgrade-using.png"));
  // Narrow: the version line wraps under the title rather than clipping.
  await win.resize(640, 800).catch(() => {});
  await win.screenshot(path.join(ctx.world.artifacts, "upgrade-narrow.png"));
  await win.fill().catch(() => {});

  await win.click({ role: "button", name: "Go Back to v3.49.0" });
  await win.waitFor({ role: "statictext", name: "Better-T-Stack v3.49.0" });
  await win.waitFor({ role: "button", name: "Upgrade to v3.50.0" });
  await win.waitFor({ name: /Mongoose/ });
});
