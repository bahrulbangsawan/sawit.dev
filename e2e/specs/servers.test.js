// The browser's list of local servers: two dev servers listening show in it, by port, and choosing
// one opens it in the tab.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import net from "node:net";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { windowState } from "../lib/sawit.js";

async function browser(ctx) {
  return (await windowState(ctx)).browser;
}

/** A free loopback port. */
function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.once("error", reject);
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

/** A dev server of its own (node, in the fixture project), whose page is titled after its port. */
function devServer(ctx, port) {
  const code =
    `require('http').createServer((q, s) => s.end('<title>Server ${port}</title><h1>${port}</h1>'))` +
    `.listen(${port}, '127.0.0.1')`;
  return spawn(process.execPath, ["-e", code], { cwd: ctx.world.project, stdio: "ignore" });
}

sawitTest("servers: the address bar's list shows the dev servers running, and opens one", { site: true }, async (ctx) => {
  const { win } = ctx;
  const ports = [await freePort(), await freePort()];
  const kids = ports.map((p) => devServer(ctx, p));
  try {
    await ctx.call({ cmd: "open_page", page: "browser" });
    await ctx.call({ cmd: "browser", op: { op: "navigate", url: ctx.site.url } });
    await until(async () => (await browser(ctx))?.url === ctx.site.url, { timeoutMs: 20_000, what: "the fixture site" });

    // Scanned as the list opens: both servers, by port.
    await win.openMenu({ role: "button", name: "Local Servers" });
    for (const port of ports) {
      await win.waitFor({ name: new RegExp(`^localhost:${port} · `) }, { timeoutMs: 15_000 });
    }
    await win.click({ name: new RegExp(`^localhost:${ports[1]} · `) });
    const opened = await until(
      async () => {
        const b = await browser(ctx);
        return b.title === `Server ${ports[1]}` ? b : null;
      },
      { timeoutMs: 15_000, what: `the server on ${ports[1]} to open` },
    );
    assert.equal(opened.url, `http://localhost:${ports[1]}/`);
    await win.waitForGone({ name: new RegExp(`^localhost:${ports[0]} · `) });
  } finally {
    for (const k of kids) k.kill();
  }
});
