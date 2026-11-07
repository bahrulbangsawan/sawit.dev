// Agents' clicks: on a tab on screen they are real mouse events (the page sees `isTrusted`), with
// the user's pointer and keyboard left where they are; on a tab that isn't on screen, and for a
// right click or a hover, they are the page agent's synthetic events, as before.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import http from "node:http";
import path from "node:path";

import { REPO } from "../lib/app.js";
import { sawitTest } from "../lib/harness.js";
import { pointer } from "../lib/host.js";
import { until } from "../lib/wait.js";

const FIXTURE = path.join(REPO, "docs", "fixtures", "browser", "pointer.html");

async function serve() {
  const body = await readFile(FIXTURE);
  const server = http.createServer((req, res) => res.writeHead(200, { "content-type": "text/html; charset=utf-8" }).end(body));
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  return { url: `http://127.0.0.1:${server.address().port}/pointer.html`, close: () => new Promise((r) => server.close(r)) };
}

// The control socket now and then closes with no reply under load: that request is sent again.
async function call(ctx, request) {
  for (let i = 0; ; i++) {
    try {
      return await ctx.call(request, { timeoutMs: 30_000 });
    } catch (e) {
      if (i >= 2 || !/^unexpected reply to [a-z_]+: ""$/.test(e.message)) throw e;
    }
  }
}
const browser = (ctx, op, tab) => call(ctx, { cmd: "browser", op, ...(tab ? { tab } : {}) });
const log = async (ctx, tab) => JSON.parse((await browser(ctx, { op: "evaluate", script: "JSON.stringify(__log.splice(0))" }, tab)).value);

async function load(ctx, url, tab) {
  if (!tab) await call(ctx, { cmd: "open_page", page: "browser" });
  await browser(ctx, { op: "navigate", url }, tab);
  await until(
    async () => {
      const s = await browser(ctx, { op: "state" }, tab);
      return !s.loading && s.url === url && s.title === "Pointer · fixture" ? s : null;
    },
    { timeoutMs: 20_000, what: "the pointer fixture to load" },
  );
}

sawitTest("pointer: an agent's click on a tab on screen is real events", async (ctx) => {
  const site = await serve();
  try {
    await load(ctx, site.url);
    const before = await pointer().catch(() => null);
    await log(ctx);

    const clicked = await browser(ctx, { op: "click", selector: "#once" });
    assert.match(clicked.clicked, /Click me/);
    let events = await log(ctx);
    const click = events.find((e) => e.type === "click" && e.target === "once");
    assert.ok(click, JSON.stringify(events));
    assert.equal(click.trusted, true, "the click is the browser's own");
    assert.ok(events.some((e) => e.type === "mousedown" && e.trusted && e.detail === 1));

    await browser(ctx, { op: "click", selector: "#twice", double: true });
    events = await log(ctx);
    const dbl = events.find((e) => e.type === "dblclick");
    assert.ok(dbl && dbl.trusted && dbl.detail === 2, JSON.stringify(events));
    assert.equal(events.filter((e) => e.type === "click").length, 2);

    // A hover stays the page agent's synthetic events, as before.
    await browser(ctx, { op: "hover", selector: "#hover" });
    events = await log(ctx);
    assert.ok(events.some((e) => e.type === "mousemove" && e.target === "hover"), JSON.stringify(events));

    // Far down the page: scrolled into view, then clicked for real.
    await browser(ctx, { op: "click", selector: "#far" });
    events = await log(ctx);
    assert.ok(events.some((e) => e.type === "click" && e.target === "far" && e.trusted), JSON.stringify(events));

    // A right click stays the page agent's (no native menu), as before.
    await browser(ctx, { op: "click", selector: "#once", button: "right" });
    events = await log(ctx);
    const menu = events.find((e) => e.type === "contextmenu");
    assert.ok(menu && menu.trusted === false, JSON.stringify(events));

    // The user's pointer stayed where it was.
    const after = await pointer().catch(() => null);
    if (before && after) assert.deepEqual(after, before);
  } finally {
    await site.close();
  }
});

sawitTest("pointer: a tab that isn't on screen gets the page agent's events", async (ctx) => {
  const site = await serve();
  try {
    await load(ctx, site.url);
    const opened = await call(ctx, { cmd: "browser_tabs", action: "open", url: site.url });
    const tab = opened.opened;
    assert.ok(tab !== undefined, JSON.stringify(opened));
    await until(
      async () => {
        const s = await browser(ctx, { op: "state" }, String(tab));
        return !s.loading && s.title === "Pointer · fixture" ? s : null;
      },
      { timeoutMs: 20_000, what: "the background tab to load" },
    );
    await log(ctx, String(tab));
    await browser(ctx, { op: "click", selector: "#once" }, String(tab));
    const events = await log(ctx, String(tab));
    const click = events.find((e) => e.type === "click" && e.target === "once");
    assert.ok(click && click.trusted === false, JSON.stringify(events));
  } finally {
    await site.close();
  }
});
