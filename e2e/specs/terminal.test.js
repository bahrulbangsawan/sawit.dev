// The terminal: a new tab runs what is typed into it; a tab can be renamed; the pane menu's keys
// do what its items do; a file dropped on it types its quoted path.
//
// The strip's tabs read as radio buttons in the "Terminals" tab list; the terminal itself as a
// text area named "Terminal". Its text is read back over the control socket (terminal_read).

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { terminalText, waitForTerminalLine, windowState } from "../lib/sawit.js";

const run = promisify(execFile);

/** The terminal tabs, as `app_state` lists them. */
async function terminals(ctx) {
  return (await windowState(ctx)).terminals;
}

/** Opens a new terminal tab from the strip's + button and waits for its shell's prompt. */
async function newTerminal(ctx) {
  const before = (await terminals(ctx)).length;
  await ctx.win.click({ role: "button", name: "New Terminal" });
  await until(async () => (await terminals(ctx)).length === before + 1, { what: "a new terminal tab" });
  // The prompt the fixture home's .zshrc sets: the shell is ready for input.
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), {
    timeoutMs: 20_000,
    what: "the new shell's prompt",
  });
  return ctx.win.waitFor({ role: "radiobutton", within: "Terminals", value: true });
}

/** The terminal text area, by its name. */
const TERMINAL = { role: /^text/, name: "Terminal" };

sawitTest("terminal: a new tab runs what is typed, and its output reads back", async (ctx) => {
  const { win } = ctx;
  const tab = await newTerminal(ctx);
  assert.equal(tab.name, "zsh", "the new tab is named after its shell");
  assert.equal((await win.findAll({ role: "radiobutton", within: "Terminals" })).length, 2);
  await win.typeKeys("echo e2e-ok");
  await win.press("return");
  const text = await waitForTerminalLine(ctx, "e2e-ok");
  assert.match(text, /echo e2e-ok/);
});

sawitTest("terminal: ⇧⌘I renames the tab, and the strip shows the name", async (ctx) => {
  const { win } = ctx;
  await newTerminal(ctx);
  await win.press("cmd+shift+i");
  // The field opens on the tab, holding its name.
  const field = { role: "textfield", name: "Tab Name" };
  assert.equal((await win.waitFor(field)).value, "zsh");
  await win.setValue(field, "e2e-renamed");
  await win.press("return");
  await win.waitForGone(field);
  await win.waitFor({ role: "radiobutton", name: "e2e-renamed", within: "Terminals", value: true });
  await until(async () => (await terminals(ctx)).some((t) => t.title === "e2e-renamed"), {
    what: "app_state to report the tab by its new name",
  });
});

sawitTest(
  "terminal: right-click a tab, Rename Tab, type a name, and the tab shows it",
  // A real right-click: GPUI's menus have no accessibility action that opens them.
  { headed: true },
  async (ctx) => {
    const { win } = ctx;
    const tab = await newTerminal(ctx);
    await win.rightClick({ role: "radiobutton", name: tab.name, within: "Terminals", value: true });
    await win.waitFor({ role: "menuitem", name: "Rename Tab" });
    const items = (await win.findAll({ role: "menuitem" })).filter((i) => i.name);
    assert.equal(items[0].name, "Rename Tab", "Rename Tab is the menu's first item");
    await win.click({ role: "menuitem", name: "Rename Tab" });
    const field = { role: "textfield", name: "Tab Name" };
    await win.setValue(field, "from-the-menu");
    await win.press("return");
    await win.waitFor({ role: "radiobutton", name: "from-the-menu", within: "Terminals" });
  },
);

/** The pane menu's items, the actions they run, and the keys the menu shows beside them. */
const PANE_MENU = [
  ["Rename pane", "terminal::RenamePane", "cmd-i"],
  ["Split right", "terminal::SplitRight", "cmd-d"],
  ["Split down", "terminal::SplitDown", "cmd-shift-d"],
  ["Zoom", "terminal::ZoomPane", "cmd-shift-enter"],
  ["Send right-clicks to pane", "terminal::ToggleSendRightClicks", "cmd-alt-r"],
  ["Close pane", "terminal::ClosePane", "alt-cmd-w"],
];

/** Whether `a` and `b` name one key combo, whatever order their modifiers are in. */
const sameKeys = (a, b) => a.split("-").sort().join("-") === b.split("-").sort().join("-");

sawitTest("terminal: the pane menu's keys are bound, and pressing them does what the items do", async (ctx) => {
  const { win } = ctx;
  // The keys beside the pane menu's items are the keymap's bindings of their actions. (The menu
  // draws them as text, which GPUI keeps out of the accessibility tree: the test reads the
  // bindings, then presses them.)
  const { rows } = await ctx.call({ cmd: "keymap", op: { op: "list", filter: "terminal" } });
  for (const [label, action, keys] of PANE_MENU) {
    const row = rows.find((r) => r.action === action && sameKeys(r.keys, keys));
    assert.ok(row, `${label}: ${action} is bound to ${keys}`);
  }

  await newTerminal(ctx);
  const panes = async () => (await terminals(ctx)).find((t) => t.active)?.panes?.length ?? 0;
  assert.equal(await panes(), 1);
  await win.press("cmd+d");
  await until(async () => (await panes()) === 2, { what: "⌘D to split the pane right" });
  await win.press("cmd+shift+d");
  await until(async () => (await panes()) === 3, { what: "⇧⌘D to split the pane down" });
  await win.press("alt+cmd+w");
  await until(async () => (await panes()) === 2, { what: "⌥⌘W to close the pane" });
  // ⌘I: a field to name the pane opens in the pane's header.
  await win.press("cmd+i");
  const field = { role: "textfield", name: "Pane Name" };
  await win.waitFor(field);
  await win.setValue(field, "left");
  await win.press("return");
  await until(
    async () => (await terminals(ctx)).find((t) => t.active)?.panes?.some((p) => p.title === "left"),
    { what: "the pane to be named “left”" },
  );
});

sawitTest("terminal: ⌘] and ⌘[ step to the next and previous pane, wrapping around", async (ctx) => {
  const { win } = ctx;
  await newTerminal(ctx);
  const focused = async () =>
    (await terminals(ctx)).find((t) => t.active)?.panes?.findIndex((p) => p.focused);
  await win.press("cmd+d");
  await until(async () => (await focused()) === 1, { what: "⌘D to split, the new pane focused" });
  await win.press("cmd+[");
  await until(async () => (await focused()) === 0, { what: "⌘[ to step back to the first pane" });
  await win.press("cmd+]");
  await until(async () => (await focused()) === 1, { what: "⌘] to step to the second pane" });
  await win.press("cmd+]");
  await until(async () => (await focused()) === 0, { what: "⌘] to wrap to the first pane" });
  // The focused pane is the selected one, as assistive technology reads the panes.
  const panes = await win.findAll({ role: "group", name: "Terminal pane" });
  assert.equal(panes.length, 2);
});

sawitTest(
  "terminal: right-click on a pane opens its menu, every item there",
  { headed: true },
  async (ctx) => {
    const { win } = ctx;
    await newTerminal(ctx);
    await win.rightClick(TERMINAL);
    await win.waitFor({ role: "menuitem", name: "Rename pane" });
    const names = (await win.findAll({ role: "menuitem" })).map((m) => m.name);
    for (const [label] of PANE_MENU) assert.ok(names.includes(label), `${label} in ${names.join(", ")}`);
    await win.press("escape");
    await win.waitForGone({ role: "menuitem", name: "Rename pane" });
  },
);

sawitTest(
  "terminal: a file dropped on the terminal types its quoted path",
  // A drag between apps: the pointer moves, and the drag starts in another app.
  { headed: true },
  async (ctx) => {
    const helper = process.env.SAWIT_E2E_DRAG_SOURCE;
    assert.ok(helper, "tools/drag-source.swift is built (run.sh does it with swiftc for --headed)");
    const { win, world } = ctx;
    await newTerminal(ctx);
    // Typing first, as the bug this guards against needed: the last input a key, then a drop.
    await win.typeKeys("ls -l ");
    const pane = await win.waitFor(TERMINAL, { bounds: true });
    const { x, y, width, height } = pane.bounds;
    const target = { x: x + width / 2, y: y + height / 2 };
    // The source sits beside the terminal, inside the screen.
    const source = { x: Math.max(80, x + width / 2 - 260), y: Math.max(80, y - 90) };
    const file = path.join(world.project, "notes.txt");
    const { stdout } = await run(
      helper,
      [file, source.x, source.y, target.x, target.y].map((v) => String(typeof v === "number" ? Math.round(v) : v)),
      { timeout: 30_000 },
    );
    assert.match(stdout, /ended/);
    // The path, quoted for the shell, typed after what was typed before.
    const text = await until(
      async () => {
        const t = await terminalText(ctx);
        return t.includes(file) ? t : null;
      },
      { what: `the dropped path in the terminal` },
    );
    assert.match(text, /ls -l .*notes\.txt/);
    await win.press("ctrl+c");
  },
);
