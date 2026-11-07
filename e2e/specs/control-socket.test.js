// The control socket answers every request. Under load, the client's close of its sending half
// could reach the app seconds after the request itself; the app gave up reading after five seconds
// and closed the connection with no answer ("unexpected reply: \"\"", about 1 call in 70). Now a
// whole request is handled as soon as it has come, close or not (crates/app/src/cli/server.rs).

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { openCanvas } from "../lib/sawit.js";

sawitTest("control socket: hundreds of requests in a row each get an answer", { site: true, timeout: 300_000 }, async (ctx) => {
  await ctx.call({ cmd: "open_page", page: "browser" });
  await ctx.call({ cmd: "browser", op: { op: "navigate", url: ctx.site.url } });
  await openCanvas(ctx);
  const ops = [
    () => ctx.call({ cmd: "state" }),
    () => ctx.call({ cmd: "browser", op: { op: "evaluate", script: "return 1" } }),
    () => ctx.call({ cmd: "canvas", op: { op: "web", action: "status" } }),
  ];
  const failures = [];
  for (let i = 0; i < 100; i++) {
    for (const op of ops) {
      const started = Date.now();
      await op().catch((e) => failures.push(`${i}: ${e.message} after ${Date.now() - started} ms`));
    }
  }
  assert.deepEqual(failures, []);
});
