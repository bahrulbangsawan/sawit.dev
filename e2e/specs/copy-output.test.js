// Copy Last Output and Copy Error (backlog item 1): ⇧⌘C copies the last command's output as plain
// text (no escape codes, lines in order), ⌥⇧⌘C only its error lines with a few around each.

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { terminalText, windowState, waitForTerminalLine } from "../lib/sawit.js";

const run = promisify(execFile);
const clipboard = async () => (await run("/usr/bin/pbpaste")).stdout;

sawitTest("copy output: ⇧⌘C copies the last command's output as plain text", async (ctx) => {
  const { win } = ctx;
  const before = (await windowState(ctx)).terminals.length;
  await win.click({ role: "button", name: "New Terminal" });
  await until(async () => (await windowState(ctx)).terminals.length === before + 1, { what: "a new terminal" });
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), { timeoutMs: 20_000, what: "the prompt" });
  // Ten lines, the fifth an error in red.
  await win.typeKeys("for i in 1 2 3 4 5 6 7 8 9 10; do if [ $i = 5 ]; then printf '\\033[31merror: five\\033[0m\\n'; else echo line-$i; fi; done");
  await win.press("return");
  await waitForTerminalLine(ctx, "line-10");
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), { what: "the prompt after it" });

  await win.press("cmd+shift+c");
  const all = await until(async () => {
    const text = await clipboard();
    return text.includes("line-10") ? text : null;
  }, { what: "the output on the clipboard" });
  const lines = all.trimEnd().split("\n");
  assert.equal(lines.length, 10, all);
  assert.equal(lines[0], "line-1");
  assert.equal(lines[4], "error: five", "no escape codes");
  assert.ok(!all.includes("\u001b"));

  await win.press("alt+cmd+shift+c");
  const errors = await until(async () => {
    const text = await clipboard();
    return text !== all ? text : null;
  }, { what: "the errors on the clipboard" });
  assert.equal(errors.trimEnd(), "line-3\nline-4\nerror: five\nline-6\nline-7");
});
