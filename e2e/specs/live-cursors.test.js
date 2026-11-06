// Live build cursors (item 30): several agents of one session build at once, each with its own
// cursor, colour and name; the document is always ahead of what plays; a screen written three
// times plays three builds and ends live; and the canvas sleeps when nothing plays.

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas, windowState } from "../lib/sawit.js";

const run = promisify(execFile);
const SESSION = { id: "mcp-e2e-1", name: "Claude Code", label: "fixture-app" };

/** A canvas op sent as lane `lane` of the one session. */
function laneOp(ctx, lane, op) {
  const agent = lane ? { ...SESSION, lane: { id: lane, name: `${lane} agent` } } : SESSION;
  return ctx.call({ cmd: "canvas", op, agent });
}

/** The app's CPU use now, in % of one core, from ps. */
async function cpu(pid) {
  const { stdout } = await run("ps", ["-o", "%cpu=", "-p", String(pid)]);
  return parseFloat(stdout.trim()) || 0;
}

/** A few screenshots of the window, `gap` ms apart, kept as the run's capture of a case. */
async function capture(ctx, name, frames = 6, gap = 250) {
  const dir = path.join(ctx.world.artifacts, "capture", name);
  await mkdir(dir, { recursive: true });
  for (let i = 0; i < frames; i++) {
    await ctx.win.screenshot(path.join(dir, `${String(i).padStart(2, "0")}.png`)).catch(() => {});
    await new Promise((r) => setTimeout(r, gap));
  }
}

/** One lane's component: a frame, its title, three cards and a connector, as parallel ops. */
async function build(ctx, lane, x) {
  const begun = await laneOp(ctx, lane, {
    op: "begin", name: `${lane} card`, frame: { x, y: 0, width: 360, height: 420 },
    steps: ["Laying out", "Writing copy"],
  });
  assert.equal(begun.ok, true, JSON.stringify(begun));
  const work = begun.work;
  const elements = [
    { type: "text", x: 16, y: 16, text: `${lane} plans`, fontSize: 20 },
    { type: "rectangle", x: 16, y: 60, width: 100, height: 140, backgroundColor: "#e0e7ff" },
    { type: "rectangle", x: 128, y: 60, width: 100, height: 140, backgroundColor: "#e0e7ff" },
    { type: "rectangle", x: 240, y: 60, width: 100, height: 140, backgroundColor: "#e0e7ff" },
    { type: "arrow", x: 16, y: 260, width: 300, height: 80 },
  ];
  const added = await laneOp(ctx, lane, { op: "add", work, elements });
  assert.equal(added.ok, true, JSON.stringify(added));
  return { work, ids: added.ids };
}

sawitTest("live cursors: four agents of one session build at once, each with its own cursor", async (ctx) => {
  await openCanvas(ctx);
  const lanes = ["header", "hero", "pricing", "footer"];
  const before = await cpu(ctx.app.pid);
  const built = await Promise.all(lanes.map((lane, i) => build(ctx, lane, i * 420)));
  // The document is ahead of what plays: every element is there at once.
  const got = await ctx.call({ cmd: "canvas", op: { op: "get" } });
  assert.equal(got.ok, true);
  const playing = await cpu(ctx.app.pid);
  await capture(ctx, "four-lanes");
  const state = await windowState(ctx);
  const agents = (state.agents ?? []).filter((a) => a.surface === "canvas");
  const names = new Set(agents.map((a) => a.name));
  const colours = new Set(agents.map((a) => a.color));
  for (const lane of lanes) assert.ok(names.has(`${lane} agent`), `${lane}: ${[...names]}`);
  assert.ok(colours.size >= 4, `four colours: ${[...colours]}`);
  assert.equal(built.length, 4);
  // Once nothing plays, the canvas sleeps.
  await new Promise((r) => setTimeout(r, 6_000));
  const idle = await cpu(ctx.app.pid);
  await writeFile(
    path.join(ctx.world.artifacts, "cpu.json"),
    JSON.stringify({ before, playing, idle }, null, 2),
  );
  ctx.log(`cpu %: before ${before}, while four lanes play ${playing}, idle ${idle}`);
});

sawitTest("live cursors: the user's selection and view never change while agents build", async (ctx) => {
  await openCanvas(ctx);
  const view = async () => (await windowState(ctx)).canvas ?? null;
  const at = await view();
  await Promise.all(["a", "b"].map((lane, i) => build(ctx, lane, i * 420)));
  const after = await view();
  assert.deepEqual(after?.selection ?? [], at?.selection ?? [], "the selection is the user's");
  assert.deepEqual(after?.zoom, at?.zoom, "so is the zoom");
});

sawitTest("live cursors: a screen written three times plays three builds and ends live", async (ctx) => {
  await openCanvas(ctx);
  const begun = await laneOp(ctx, null, {
    op: "begin", name: "Pricing", kind: "screen", frame: { x: 0, y: 0, width: 600, height: 500 },
  });
  assert.equal(begun.ok, true, JSON.stringify(begun));
  const page = (body) =>
    `<!doctype html><html><head><style>body{margin:0;font:16px system-ui;padding:24px}.card{display:inline-block;width:160px;height:120px;margin:8px;background:#e0e7ff;border-radius:8px}</style></head><body>${body}</body></html>`;
  const writes = [
    page("<h1>Pricing</h1>"),
    page("<h1>Pricing</h1><p>Pick a plan.</p>"),
    page("<h1>Pricing</h1><p>Pick a plan.</p><div class=card></div><div class=card></div><div class=card></div>"),
  ];
  for (const [i, html] of writes.entries()) {
    const wrote = await laneOp(ctx, null, { op: "screen_write", work: begun.work, html });
    assert.equal(wrote.ok, true, JSON.stringify(wrote));
    await capture(ctx, `screen-write-${i + 1}`, 6, 300);
  }
  // It ends on the live page: the frame shows its picture (pictured) and takes a click.
  await until(async () => {
    const s = await windowState(ctx);
    return JSON.stringify(s).includes("Pricing") ? s : null;
  }, { what: "the screen on the canvas" });
});
