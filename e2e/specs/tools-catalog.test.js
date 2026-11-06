// The Dev Tools rows added for the GitHub CLI, MinIO, Herdr, HyperFrames and voxl: each shows,
// with its state, and the buttons its row adds are there. Nothing is pressed that installs,
// signs in or starts anything: desktop.js refuses those buttons outright.
//
// The home is a throwaway one, but Homebrew's /opt/homebrew is this Mac's: a gh or mc installed
// there shows as installed, and the GitHub CLI isn't signed in (its sign-in lives in the home).

import assert from "node:assert/strict";
import { existsSync } from "node:fs";

import { sawitTest } from "../lib/harness.js";
import { openPage, waitForTab } from "../lib/sawit.js";

async function search(win, text) {
  await win.setValue({ role: "textfield", name: "Search tools" }, text);
}

sawitTest("tools: the GitHub CLI shows its gh sign-in and offers Sign In with GitHub CLI", async (ctx) => {
  const { win } = ctx;
  await openPage(ctx, "Dev Tools");
  await waitForTab(ctx, "tools");
  await search(win, "github");
  await win.waitFor({ role: "group", name: "GitHub CLI" });
  if (existsSync("/opt/homebrew/bin/gh") || existsSync("/usr/local/bin/gh")) {
    await win.waitFor({ role: "statictext", name: /^Installed · \d+\.\d+/, within: "GitHub CLI" });
    // A throwaway home has no gh sign-in.
    await win.waitFor(
      { role: "statictext", name: /GitHub CLI isn't signed in/, within: "GitHub CLI" },
      { timeoutMs: 30_000 },
    );
    await win.waitFor({ role: "button", name: "Sign In with GitHub CLI", within: "GitHub CLI" });
  } else {
    await win.waitFor({ role: "button", name: "Install", within: "GitHub CLI" });
  }
});

sawitTest("tools: MinIO and its client, Herdr and Herdr TUI, HyperFrames and voxl are listed", async (ctx) => {
  const { win } = ctx;
  await openPage(ctx, "Dev Tools");
  await waitForTab(ctx, "tools");
  await search(win, "minio");
  await win.waitFor({ role: "group", name: "MinIO" });
  await win.waitFor({ role: "group", name: "MinIO Client" });
  // A MinIO Client from Homebrew shows its dated release; Midnight Commander's mc wouldn't count.
  if (existsSync("/opt/homebrew/bin/mc")) {
    const state = await win.waitFor({ role: "statictext", name: /^(Installed|Not installed)/, within: "MinIO Client" });
    if (state.name.startsWith("Installed")) assert.match(state.name, /^Installed · \d{4}-\d{2}-\d{2}T/);
  }

  await search(win, "herdr");
  await win.waitFor({ role: "group", name: "Herdr" });
  await win.waitFor({ role: "group", name: "Herdr TUI" });
  // This Mac has Apple silicon: the app is offered, not marked unsupported.
  if (process.arch === "arm64") {
    await win.waitForGone({ role: "statictext", name: "Needs an Apple Silicon Mac" });
  }

  await search(win, "hyperframes");
  await win.waitFor({ role: "group", name: "HyperFrames CLI" });
  await win.waitFor({ role: "group", name: "HyperFrames Studio" });
  // It needs Node.js and FFmpeg, shown as chips.
  await win.waitFor({ role: "statictext", name: "needs FFmpeg", within: "HyperFrames CLI" });

  await search(win, "voxl");
  await win.waitFor({ role: "group", name: "voxl" });
  await win.waitFor({ role: "button", name: "Install in Terminal", within: "voxl" });
});
