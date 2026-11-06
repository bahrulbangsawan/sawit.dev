// Who has the keyboard. The user moves it, nothing else does: after the browser's page had the
// keyboard, a terminal the user opens gets every key; and while the user types in a terminal, an
// agent's browser and canvas work (navigate, type, click, press, a live web frame) never takes a
// key from it. Each key the page receives is logged by the fixture site (`window.__keys`).

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { terminalText, waitForTerminalLine, windowState } from "../lib/sawit.js";

/** The browser tab's state, as `app_state` reports it. */
async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

/** The keys the fixture page has received since it loaded. */
async function pageKeys(ctx) {
  const r = await ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return window.__keys" } });
  return r?.value ?? [];
}

/** Opens the browser on the fixture site by typing its address: Return gives the page the keyboard. */
async function openSite(ctx) {
  const { win, site } = ctx;
  await ctx.call({ cmd: "open_page", page: "browser" });
  await win.setValue({ role: "textfield", name: "Search or enter address" }, site.url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.pressInPage("return");
  await until(
    async () => {
      const b = await browser(ctx);
      return b?.url === site.url && !b.loading ? b : null;
    },
    { timeoutMs: 20_000, what: "the fixture site to load" },
  );
}

/**
 * Opens a terminal tab as the user does, with the terminal dock's New Terminal (⌘T is the
 * browser's New Tab while the browser has the keyboard), and waits for its prompt.
 */
async function userTerminal(ctx) {
  const before = (await windowState(ctx)).terminals.length;
  await ctx.win.click({ role: "button", name: "New Terminal" });
  await until(async () => (await windowState(ctx)).terminals.length === before + 1, { what: "⌘T's terminal" });
  await until(async () => /e2e%\s*$/.test(await terminalText(ctx)), { timeoutMs: 20_000, what: "its prompt" });
  return (await windowState(ctx)).terminals.find((t) => t.active).index;
}

/**
 * Brings the test's window to the front first: the Mac's own activity (a notification, another
 * app) can take key status mid-test, and agent-desktop then sends no key at all (STALE_REF). It
 * never moves the keyboard inside the window.
 */
async function front(ctx) {
  await ctx.desktop.call(["focus-window", "--window-id", ctx.win.id]);
}

/** Types `echo <word>` and Return as the user, and expects the line in terminal `tab`. */
async function typeInTerminal(ctx, tab, word) {
  await front(ctx);
  await ctx.win.typeKeys(`echo ${word}`);
  await ctx.win.press("return");
  await waitForTerminalLine(ctx, word, { tab, timeoutMs: 8_000 });
}

sawitTest("focus: after the page had the keyboard, the terminal the user opens gets every key", { site: true }, async (ctx) => {
  await openSite(ctx);
  // The page has the keyboard: a key goes to it.
  await ctx.win.pressInPage("q");
  await until(async () => (await pageKeys(ctx)).includes("q"), { what: "the page to get the key" });
  const tab = await userTerminal(ctx);
  const before = (await pageKeys(ctx)).length;
  for (let round = 0; round < 3; round++) {
    await typeInTerminal(ctx, tab, `focus-${round}`);
    // Tab, as completion: it must reach the shell, not move the page's focus.
    await ctx.win.press("tab");
  }
  assert.equal((await pageKeys(ctx)).length, before, "no key the user typed in the terminal reached the page");
});

sawitTest("focus: an agent's browser and canvas work leaves the user's terminal its keys", { site: true, timeout: 300_000 }, async (ctx) => {
  const { site } = ctx;
  await openSite(ctx);
  const tab = await userTerminal(ctx);
  await typeInTerminal(ctx, tab, "user-start");

  const agent = {
    navigate: () => ctx.call({ cmd: "browser", op: { op: "navigate", url: site.url } }),
    type: () => ctx.call({ cmd: "browser", op: { op: "type", selector: "#field", text: "agent" } }),
    click: () => ctx.call({ cmd: "browser", op: { op: "click", selector: "#title" } }),
    press: () => ctx.call({ cmd: "browser", op: { op: "press", key: "Tab" } }),
    terminal_run: () => ctx.call({ cmd: "terminal_run", command: "echo agent-ran", new_tab: true }),
    canvas_page: () => ctx.call({ cmd: "open_page", page: "canvas" }),
    canvas_routes: () =>
      ctx.call({ cmd: "canvas", op: { op: "routes", server: site.url, routes: ["/"], viewports: [1280] } }),
    canvas_enter: async () => {
      const status = await ctx.call({ cmd: "canvas", op: { op: "web", action: "status" } });
      const id = status?.frames?.[0]?.id ?? status?.frames?.[0];
      return ctx.call({ cmd: "canvas", op: { op: "web", action: "enter", id } });
    },
    browser_page: () => ctx.call({ cmd: "open_page", page: "browser" }),
  };
  for (const [name, act] of Object.entries(agent)) {
    await act();
    // Let the agent's work land (a load, a frame going live) before the user's next key.
    await new Promise((r) => setTimeout(r, 1_500));
    await typeInTerminal(ctx, tab, `after-${name.replace("_", "-")}`);
  }
  const keys = await pageKeys(ctx).catch(() => []);
  assert.ok(!keys.some((k) => k === "e" || k === "c"), `the page got none of the user's keys: ${keys.join(" ")}`);
  const text = await terminalText(ctx, { tab, lines: 200 });
  assert.match(text, /after-browser-page/);
});

sawitTest("focus: twenty rounds between the page and the terminal, every key lands where the user is", { site: true, timeout: 300_000 }, async (ctx) => {
  const { win } = ctx;
  await openSite(ctx);
  const tab = await userTerminal(ctx);
  const address = { role: "textfield", name: "Search or enter address" };
  for (let round = 0; round < 20; round++) {
    // To the page, as the user does from the address field: Return gives it the keyboard.
    await win.focus(address);
    await win.press("return");
    await until(async () => !(await browser(ctx)).loading, { what: "the reload" });
    await win.pressInPage("z");
    await until(async () => (await pageKeys(ctx)).includes("z"), { what: `round ${round}: the page's key` });
    // Back to the terminal, as a click on it does (accessibility focus), then Tab and a line.
    await win.focus({ role: /^text/, name: "Terminal" });
    await win.press("tab");
    await typeInTerminal(ctx, tab, `round-${round}`);
    assert.ok(!(await pageKeys(ctx)).some((k) => k === "o" || k === "Tab"), `round ${round}: the page got none of the terminal's keys`);
  }
});

sawitTest("focus: ⌃Tab switches tabs while the page has the keyboard", { site: true }, async (ctx) => {
  const { win } = ctx;
  // A second tab for ⌃Tab to go to.
  await ctx.call({ cmd: "open", path: `${ctx.world.project}/README.md` });
  await until(async () => (await windowState(ctx)).tabs.length === 1, { what: "README's tab" });
  await openSite(ctx);
  await win.pressInPage("q");
  await until(async () => (await pageKeys(ctx)).includes("q"), { what: "the page to get the key" });
  const before = (await windowState(ctx)).tabs.find((t) => t.active).index;
  await win.pressInPage("ctrl+tab");
  await until(async () => (await windowState(ctx)).tabs.find((t) => t.active).index !== before, {
    what: "⌃Tab to move to the next tab",
  });
});

sawitTest("focus: the user typing in the page keeps every key while an agent works elsewhere", { site: true }, async (ctx) => {
  const { win } = ctx;
  await openSite(ctx);
  // The user's own field in the page: Tab from the page's start reaches it.
  await win.pressInPage("tab");
  for (const [i, act] of [
    () => ctx.call({ cmd: "terminal_run", command: "echo agent-1", new_tab: true }),
    () => ctx.call({ cmd: "terminal_run", command: "echo agent-2" }),
    () => ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return document.title" } }),
  ].entries()) {
    await act();
    await new Promise((r) => setTimeout(r, 1_000));
    await win.pressInPage(String(i));
  }
  await until(async () => (await pageKeys(ctx)).filter((k) => /^[0-2]$/.test(k)).join("") === "012", {
    what: "the page to get all three of the user's keys, in order",
  });
});
