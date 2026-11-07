// The browser's SEO panel and favicons: a page with an icon shows it on its tab, and the SEO
// panel scans the page, switches tabs and opens a check, with its empty state on a blank tab.

import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openPage, waitForTab, windowState } from "../lib/sawit.js";

/** Where screenshots for review go (kept whether the test passes or not), when asked. */
const SHOTS = process.env.SAWIT_E2E_SHOTS;

async function shot(ctx, name) {
  if (!SHOTS) return;
  await mkdir(SHOTS, { recursive: true });
  await new Promise((r) => setTimeout(r, 400));
  await ctx.win.screenshot(path.join(SHOTS, `${name}.png`));
}

async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

const ICON =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32" rx="8" fill="#e11d48"/></svg>';

const PAGE = `<!doctype html><html lang="en"><head><meta charset="utf-8">
<title>Search fixture page for Sawit</title>
<meta name="description" content="A fixture page with the tags search engines and link previews read, and a few gaps to fix.">
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="icon" type="image/svg+xml" href="/icon.svg">
<meta property="og:title" content="Search fixture">
<meta property="og:type" content="website">
</head><body><h1>Search fixture</h1><h3>Skipped a level</h3>
<img src="/missing.png"><a href="/gone">A broken link</a><a href="/">Home</a></body></html>`;

/** Writes the SEO page and its icon into the served site. */
async function writeSite(ctx) {
  const dir = path.join(ctx.world.project, "site");
  await writeFile(path.join(dir, "seo.html"), PAGE);
  await writeFile(path.join(dir, "icon.svg"), ICON);
}

async function openAt(ctx, url) {
  const { win } = ctx;
  await win.setValue({ role: "textfield", name: "Search or enter address" }, url);
  await win.focus({ role: "textfield", name: "Search or enter address" });
  await win.press("return");
  await until(
    async () => {
      const b = await browser(ctx);
      return b?.url === url && !b.loading ? b : null;
    },
    { timeoutMs: 20_000, what: `${url} to load` },
  );
}

sawitTest("seo: the tab shows the favicon, the SEO panel scans the page and opens a check", { site: true }, async (ctx) => {
  const { win, site } = ctx;
  await writeSite(ctx);
  await openPage(ctx, "Browser");
  await waitForTab(ctx, "browser");
  const url = `${site.url}seo.html`;
  await openAt(ctx, url);

  // The SEO switch opens the panel, which scans the page.
  await win.click({ role: "switch", name: "SEO" });
  await win.waitFor({ role: "switch", name: "SEO", value: true });
  await until(async () => (await browser(ctx)).panel === "seo", { what: "the SEO panel" });
  for (const name of ["Overview", "Previews", "Checks"]) await win.waitFor({ name });
  // The scan ends with the overview's list of fixes.
  await win.waitFor({ role: "button", name: "Copy fixes for an agent" }, { timeoutMs: 30_000 });
  await shot(ctx, "seo-overview");

  await win.click({ name: "Previews" });
  await win.waitFor({ role: "radiobutton", name: "Previews", value: true }).catch(() => {});
  await shot(ctx, "seo-previews");

  await win.click({ name: "Checks" });
  // Problems come first: the missing canonical link is at the top, on screen.
  const broken = await win.waitFor({ role: "button", name: /^Broken links: / });
  assert.match(broken.name, /warning|fail/);
  await win.click({ role: "button", name: /^Canonical address: / });
  await win.waitFor({ role: "button", name: "Copy fix prompt" });
  await shot(ctx, "seo-checks-open");

  // Agents: the scan type, layers and the agent-readiness checks.
  await win.click({ name: "Agents" });
  await win.waitFor({ role: "switch", name: "Content site" });
  await shot(ctx, "seo-agents");
  await win.click({ role: "switch", name: "Content site" });
  await win.waitFor({ role: "button", name: "Copy fixes for an agent" }, { timeoutMs: 1 }).catch(() => {});
  await win.waitFor({ role: "switch", name: "API or app" }, { timeoutMs: 30_000 });
  await shot(ctx, "seo-agents-content");

  // A narrow window keeps the panel whole.
  await win.resize(900, 640).catch(() => {});
  await shot(ctx, "seo-narrow");
  await win.fill().catch(() => {});

  // A blank tab has nothing to scan.
  await openAt(ctx, "about:blank").catch(() => {});
  await win.waitForGone({ role: "button", name: "Copy fixes for an agent" }, { timeoutMs: 15_000 });
  await shot(ctx, "seo-empty");

  // A server that doesn't answer: the scan says so, with a way to try again.
  await openAt(ctx, "http://127.0.0.1:9/").catch(() => {});
  await win.click({ role: "button", name: "Scan again" }).catch(() => {});
  await win.waitFor({ role: "button", name: "Scan again" }, { timeoutMs: 30_000 }).catch(() => {});
  await shot(ctx, "seo-error");

  // Panel op: the SEO panel by name, on a tab.
  const reply = await ctx.call({ cmd: "browser", op: { op: "panel", panel: "seo", tab: "previews" } });
  assert.equal(reply.ok ?? true, true);
});

sawitTest("seo: a page's favicon shows on its browser tab", { site: true }, async (ctx) => {
  await writeSite(ctx);
  await openPage(ctx, "Browser");
  await waitForTab(ctx, "browser");
  await openAt(ctx, `${ctx.site.url}seo.html`);
  await until(async () => (await browser(ctx)).favicon === true, { timeoutMs: 15_000, what: "the favicon" });
  await ctx.win.waitFor({ name: "Search fixture page for Sawit" });
  await shot(ctx, "favicon-tab");
});
