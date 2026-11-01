// The browser tab: it loads the fixture site, and its device menu sizes the page as a phone.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { waitForTab, windowState } from "../lib/sawit.js";

/** The browser tab's state, as `app_state` reports it: url, title, viewport, … */
async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

sawitTest("browser: loads the fixture site, and the device menu picks iPhone 15", { site: true }, async (ctx) => {
  const { win, site } = ctx;
  await win.click({ role: "button", name: "Browser" });
  await waitForTab(ctx, "browser");

  // Typing the address: the field's value is written, then Return goes to the field.
  await win.setValue({ role: "textfield", name: "Search or enter address" }, site.url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.press("return");
  const loaded = await until(
    async () => {
      const b = await browser(ctx);
      return b?.url === site.url && !b.loading && b.title === "Fixture Site" ? b : null;
    },
    { timeoutMs: 20_000, what: "the fixture site to load" },
  );
  assert.equal(loaded.title, "Fixture Site");

  // Responsive at first: the button names the mode.
  await win.openMenu({ role: "button", name: "Device Size: Responsive" });
  await win.waitFor({ role: "menuitem", name: "Responsive" });
  await win.click({ role: "menuitem", name: /^iPhone 15 · \d+ × \d+$/ });
  const button = await win.waitFor({ role: "button", name: /^Device Size: iPhone 15 · / });
  assert.match(button.name, /393 × 852/);
  await win.waitForGone({ role: "menuitem", name: /^iPhone 15/ });
  const sized = await until(
    async () => {
      const b = await browser(ctx);
      return JSON.stringify(b?.viewport ?? {}).includes("393") ? b : null;
    },
    { what: "the page to take the phone's width" },
  );
  assert.ok(sized);
});

/** Opens the browser on the fixture site and waits for it to load. */
async function openSite(ctx) {
  const { win, site } = ctx;
  await win.click({ role: "button", name: "Browser" });
  await waitForTab(ctx, "browser");
  await win.setValue({ role: "textfield", name: "Search or enter address" }, site.url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.press("return");
  await until(
    async () => {
      const b = await browser(ctx);
      return b?.url === site.url && !b.loading ? b : null;
    },
    { timeoutMs: 20_000, what: "the fixture site to load" },
  );
}

const SEGMENTS = ["Inspect", "Colors", "Fonts", "Assets", "System"];

sawitTest("browser: Design opens its panel, whose tabs switch, and Annotate takes over", { site: true }, async (ctx) => {
  const { win } = ctx;
  await openSite(ctx);
  // The toolbar's Design and Annotate are switches: one panel at a time.
  await win.click({ role: "switch", name: "Design" });
  await win.waitFor({ role: "switch", name: "Design", value: true });
  await until(async () => (await browser(ctx)).panel === "design", { what: "the Design panel" });
  for (const name of SEGMENTS) await win.waitFor({ name });
  await win.waitFor({ role: "button", name: "Read the page again" });
  // Design inspects what is clicked in the page on its Inspect tab only: on Colors, it stops.
  const inspecting = (await browser(ctx)).pick_mode;
  await win.click({ name: "Colors" });
  await until(async () => (await browser(ctx)).pick_mode !== inspecting, {
    what: `picking to stop being ${inspecting} on the Colors tab`,
  });

  await win.click({ role: "switch", name: "Annotate" });
  await win.waitFor({ role: "switch", name: "Annotate", value: true });
  await win.waitFor({ role: "switch", name: "Design", value: false });
  const b = await until(async () => {
    const s = await browser(ctx);
    return s.panel === "annotate" ? s : null;
  }, { what: "the Annotate panel" });
  assert.equal(b.pick_mode, "annotate");
  await win.click({ role: "switch", name: "Annotate" });
  await until(async () => (await browser(ctx)).panel === null, { what: "no panel" });
});

sawitTest("browser: the panel control op opens Design on a tab of its own", { site: true }, async (ctx) => {
  const { win } = ctx;
  await openSite(ctx);
  await ctx.call({ cmd: "browser", op: { op: "panel", panel: "design", tab: "fonts" } });
  await win.waitFor({ role: "switch", name: "Design", value: true });
  await until(async () => (await browser(ctx)).panel === "design", { what: "the Design panel" });
  await win.waitFor({ name: "Fonts" });
});

sawitTest(
  "browser: Annotate picks an element in the page, and Save keeps the note",
  {
    pending:
      "the note's popover is in the page, in a closed shadow root, and picks only on trusted " +
      "input: a real click in the WKWebView (--headed) and keys typed there. WKWebView's own " +
      "accessibility tree isn't part of Sawit's window tree as agent-desktop reads it, so the " +
      "popover can't be found by name; drive it with a coordinate click on the element's " +
      "bounds (browser op query) and read the note back with the annotations op",
  },
);
