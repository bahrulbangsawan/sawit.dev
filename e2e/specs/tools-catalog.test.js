// The Dev Tools rows added for the GitHub CLI, MinIO, Herdr, HyperFrames and voxl: each shows,
// with its state, and the buttons its row adds are there. Nothing is pressed that installs,
// signs in or starts anything: desktop.js refuses those buttons outright.
//
// Rows read through accessibility as groups named after the tool, with their buttons inside
// ("Uninstall GitHub CLI", "Sign In with GitHub CLI"); their text isn't exposed.
//
// The home is a throwaway one, but Homebrew's /opt/homebrew is this Mac's: a gh or mc installed
// there shows as installed, and the GitHub CLI isn't signed in (its sign-in lives in the home).

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
    // Homebrew's copy: Sawit can remove it.
    await win.waitFor({ role: "button", name: "Uninstall GitHub CLI", within: "GitHub CLI" });
    // A throwaway home has no gh sign-in, once `gh auth status` answered.
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
  // A MinIO Client from Homebrew is found (and Sawit can remove it); Midnight Commander's mc
  // wouldn't count.
  if (existsSync("/opt/homebrew/Cellar/minio-mc") || existsSync("/opt/homebrew/Cellar/mc")) {
    await win.waitFor({ role: "button", name: "Uninstall MinIO Client", within: "MinIO Client" });
  }

  await search(win, "herdr");
  await win.waitFor({ role: "group", name: "Herdr" });
  await win.waitFor({ role: "group", name: "Herdr TUI" });
  // This Mac has Apple silicon: the app (not in a throwaway home) is offered, not marked
  // unsupported.
  if (process.arch === "arm64") {
    await win.waitFor({ role: "button", name: "Install", within: "Herdr" });
  }

  await search(win, "hyperframes");
  await win.waitFor({ role: "group", name: "HyperFrames CLI" });
  await win.waitFor({ role: "group", name: "HyperFrames Studio" });
  // Not installed in a throwaway home (nvm's Node.js isn't there): offered.
  await win.waitFor({ role: "button", name: "Install", within: "HyperFrames CLI" });

  await search(win, "voxl");
  await win.waitFor({ role: "group", name: "voxl" });
  await win.waitFor({ role: "button", name: "Install in Terminal", within: "voxl" });
});
