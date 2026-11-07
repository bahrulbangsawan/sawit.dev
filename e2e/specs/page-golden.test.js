// The page script's answers stay the same as its logic moves from JavaScript to Rust: the element
// snapshot, the design scan (every part of `browser_design`) and inspect, read on the fixture pages
// in docs/fixtures/browser/, must equal the answers kept in e2e/fixtures/golden/.
//
//   SAWIT_GOLDEN_WRITE=1   writes the answers as the new golden files instead of comparing
//   SAWIT_GOLDEN_URLS=a,b  also reads these pages (real sites) and writes their answers to
//   SAWIT_GOLDEN_OUT=dir   that folder, for comparing two builds by hand (no golden files)

import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import http from "node:http";
import path from "node:path";

import { REPO } from "../lib/app.js";
import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";

const PAGES = ["golden.html", "design.html", "design-nav.html", "form.html", "annotate.html", "pick.html"];
const PARTS = ["summary", "colors", "fonts", "assets", "tokens", "variables"];
const INSPECT = ["body", "h1", "#buy", ".ghost", "#deep", ".card", "a", "input", "#nope"];
const GOLDEN = path.join(REPO, "e2e", "fixtures", "golden");
const FIXTURES = path.join(REPO, "docs", "fixtures", "browser");
const ORIGIN = "http://fixture";

/** Serves docs/fixtures/browser on a loopback port: `{ url, close }`. */
async function serveFixtures() {
  const sockets = new Set();
  const types = { ".html": "text/html; charset=utf-8", ".svg": "image/svg+xml", ".png": "image/png" };
  const server = http.createServer(async (req, res) => {
    const rel = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const file = path.join(FIXTURES, rel);
    if (!file.startsWith(FIXTURES)) return res.writeHead(403).end();
    try {
      const body = await readFile(file);
      const type = types[path.extname(file)] || "application/octet-stream";
      res.writeHead(200, { "content-type": type, "cache-control": "no-store" }).end(body);
    } catch {
      res.writeHead(404).end();
    }
  });
  server.on("connection", (s) => {
    sockets.add(s);
    s.on("close", () => sockets.delete(s));
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    close: () =>
      new Promise((r) => {
        for (const s of sockets) s.destroy();
        server.close(r);
      }),
  };
}

const browser = (ctx, op, o) => ctx.call({ cmd: "browser", op }, { timeoutMs: 45_000, ...o });
const settle = (ms) => new Promise((r) => setTimeout(r, ms));

/** Every answer for the page at `url`, with `origin` written as `ORIGIN` and dates left out. */
async function answers(ctx, url, origin) {
  await browser(ctx, { op: "navigate", url });
  await until(
    async () => {
      const s = await browser(ctx, { op: "state" });
      return !s.loading && s.url && s.url.startsWith(url.split("#")[0]) ? s : null;
    },
    { timeoutMs: 30_000, what: `${url} to load` },
  );
  await settle(1500);
  const out = {};
  const ask = async (op) => browser(ctx, op).catch((e) => ({ error: e.message }));
  out.snapshot = await ask({ op: "snapshot" });
  for (const part of PARTS) out[`design.${part}`] = await ask({ op: "design", part });
  for (const selector of INSPECT) out[`inspect ${selector}`] = await ask({ op: "inspect", selector, format: "json" });
  out["inspect html #buy"] = await ask({ op: "inspect", selector: "#buy", format: "html" });
  let text = JSON.stringify(out, null, 1);
  if (origin) text = text.split(origin).join(ORIGIN);
  text = text.replace(/\b20\d\d-\d\d-\d\d\b/g, "DATE");
  return JSON.parse(text);
}

sawitTest("page golden: snapshot, design and inspect answer as before", { timeout: 600_000 }, async (ctx) => {
  const site = await serveFixtures();
  try {
    await browser(ctx, { op: "resize", width: 1280, height: 800 });
    const write = process.env.SAWIT_GOLDEN_WRITE === "1";
    if (write) await mkdir(GOLDEN, { recursive: true });
    const failures = [];
    for (const page of PAGES) {
      const got = await answers(ctx, `${site.url}/${page}`, site.url);
      const file = path.join(GOLDEN, page.replace(/\.html$/, ".json"));
      if (write) {
        await writeFile(file, JSON.stringify(got, null, 1) + "\n");
        continue;
      }
      const want = JSON.parse(await readFile(file, "utf8"));
      for (const key of Object.keys(want)) {
        try {
          assert.deepEqual(got[key], want[key]);
        } catch (e) {
          failures.push(`${page} ${key}:\n${e.message.slice(0, 4000)}`);
        }
      }
      await writeFile(path.join(ctx.world.artifacts, page.replace(/\.html$/, ".json")), JSON.stringify(got, null, 1));
    }
    const urls = (process.env.SAWIT_GOLDEN_URLS || "").split(",").filter(Boolean);
    const outDir = process.env.SAWIT_GOLDEN_OUT;
    if (urls.length && outDir) {
      await mkdir(outDir, { recursive: true });
      for (const url of urls) {
        const got = await answers(ctx, url, null).catch((e) => ({ error: e.message }));
        const name = url.replace(/^https?:\/\//, "").replace(/[^a-z0-9]+/gi, "-").replace(/-$/, "");
        await writeFile(path.join(outDir, `${name}.json`), JSON.stringify(got, null, 1) + "\n");
      }
    }
    assert.deepEqual(failures, [], failures.join("\n\n"));
  } finally {
    await site.close();
  }
});
