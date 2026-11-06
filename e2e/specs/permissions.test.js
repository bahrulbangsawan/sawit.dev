// The Permissions page edits Claude Code's rules in the throwaway home and project only: a rule is
// added at user level, moved to Deny, removed, and added at project level, each through the diff
// sheet's Save, and each time the file on disk is read back. The user's other keys, their order
// and the file's formatting stay; a backup is left beside it.

import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { openPage, waitForTab } from "../lib/sawit.js";

const RULE = "Bash(npm run test:*)";

const SETTINGS = `{
  "model": "opus",
  "permissions": {
    "allow": [
      "Read(~/notes/**)"
    ],
    "defaultMode": "acceptEdits"
  },
  "includeCoAuthoredBy": false
}
`;

/** The file's JSON, once `ok` holds for it (the app writes it a moment after Save). */
async function settled(file, ok) {
  const deadline = Date.now() + 10_000;
  let last = null;
  while (Date.now() < deadline) {
    if (existsSync(file)) {
      last = readFileSync(file, "utf8");
      const json = JSON.parse(last);
      if (ok(json)) return { json, text: last };
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`${file} never got there: ${last}`);
}

/** Opens the diff sheet's Save and waits for the sheet to go. */
async function save(win) {
  const sheet = await win.waitFor({ role: "dialog", name: "Save Change" });
  await win.click({ role: "button", name: "Save Change", within: sheet.name });
  await win.waitForGone({ role: "dialog", name: "Save Change" });
}

sawitTest("permissions: a Claude Code rule is added, moved and removed at user and project level", async (ctx) => {
  const { win, world } = ctx;
  const user = path.join(world.home, ".claude", "settings.json");
  mkdirSync(path.dirname(user), { recursive: true });
  writeFileSync(user, SETTINGS);
  const project = path.join(world.project, ".claude", "settings.json");

  await openPage(ctx, "Permissions");
  await waitForTab(ctx, "permissions");
  await win.waitFor({ role: "group", name: "User" });

  // Add, at user level.
  await win.setValue({ role: "textfield", name: "New Rule for User" }, RULE);
  await win.click({ role: "button", name: "Add to Allow…", within: "User" });
  await save(win);
  let { json, text } = await settled(user, (j) => j.permissions.allow.includes(RULE));
  assert.deepEqual(json.permissions.allow, ["Read(~/notes/**)", RULE]);
  // Only the rule's lines changed: the rest, its order and its formatting are the user's.
  assert.equal(
    text,
    SETTINGS.replace('      "Read(~/notes/**)"\n', `      "Read(~/notes/**)",\n      "${RULE}"\n`),
  );
  assert.equal(readFileSync(`${user}.sawit-backup`, "utf8"), SETTINGS);

  // Move it to Deny.
  await win.openMenu({ role: "button", name: RULE, within: "User" });
  await win.click({ role: "menuitem", name: "Move to Deny…" });
  await save(win);
  ({ json } = await settled(user, (j) => (j.permissions.deny ?? []).includes(RULE)));
  assert.deepEqual(json.permissions.allow, ["Read(~/notes/**)"]);
  assert.deepEqual(json.permissions.deny, [RULE]);
  assert.equal(json.permissions.defaultMode, "acceptEdits");
  assert.deepEqual(Object.keys(json), ["model", "permissions", "includeCoAuthoredBy"]);

  // Remove it.
  await win.openMenu({ role: "button", name: RULE, within: "User" });
  await win.click({ role: "menuitem", name: "Remove Rule…" });
  await save(win);
  ({ json } = await settled(user, (j) => !(j.permissions.deny ?? []).includes(RULE)));
  assert.deepEqual(json.permissions.deny, []);

  // Add at project level: the project's file isn't there yet, and Save creates it.
  assert.equal(existsSync(project), false);
  const group = "Project · fixture-app";
  await win.waitFor({ role: "group", name: group });
  await win.setValue({ role: "textfield", name: `New Rule for ${group}` }, "mcp__sawit__execute_sql");
  await win.click({ role: "button", name: "Add to Ask…", within: group });
  await save(win);
  ({ json } = await settled(project, (j) => (j.permissions?.ask ?? []).includes("mcp__sawit__execute_sql")));
  assert.deepEqual(json, { permissions: { ask: ["mcp__sawit__execute_sql"] } });
  // The page reads it back: Sawit's tool now asks because of that rule.
  await win.waitFor({ role: "group", name: "execute_sql" });
});
