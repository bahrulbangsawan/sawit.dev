// The design standard's machine checks on the canvas: what an agent's screen write fails shows as
// a badge on the screen's frame, which the user opens for the list; a check that passes takes the
// badge away. sawit-mcp sends the checks with the `checks` op, as this test does.

import assert from "node:assert/strict";

import { sawitTest } from "../lib/harness.js";
import { openCanvas } from "../lib/sawit.js";

const FAILURES = [
  { rule: "col.10", selector: "p.note", fix: "Text #aaaaaa on #ffffff is 2.32:1; it needs 4.5:1." },
  { rule: "typ.11", selector: "p.legal", fix: "Text is 10px; the floor is 11px." },
  { rule: "a11y.2", selector: "button.save", fix: "The target is 128 × 30px; it needs 44 × 44px here." },
];

sawitTest("design checks: a screen's failures show as a badge that opens their list", async (ctx) => {
  const { win } = ctx;
  await openCanvas(ctx);
  const begun = await ctx.call({
    cmd: "canvas",
    op: { op: "begin", name: "Account", kind: "screen", frame: { x: 0, y: 0, width: 393, height: 852 } },
  });
  assert.equal(begun.ok, true, JSON.stringify(begun));
  // The work is done (its row stops animating), then sawit-mcp's checks arrive.
  await ctx.call({ cmd: "canvas", op: { op: "progress", work: begun.work, status: "done" } });
  const checked = await ctx.call({ cmd: "canvas", op: { op: "checks", frame: begun.frame, findings: FAILURES } });
  assert.equal(checked.ok, true, JSON.stringify(checked));
  assert.equal(checked.findings.length, 3);
  // The agent's check doesn't open the list or take the keyboard: the user does.
  await win.click({ role: "button", name: "3 design issues" });
  await win.waitFor({ role: "group", name: "Design standard: what fails" });
  await win.click({ role: "button", name: "Close" });
  await win.waitForGone({ role: "group", name: "Design standard: what fails" });
  // A check that passes takes the badge away.
  const passed = await ctx.call({ cmd: "canvas", op: { op: "checks", frame: begun.frame, findings: [] } });
  assert.equal(passed.ok, true);
  await win.waitForGone({ role: "button", name: "3 design issues" });
});

sawitTest("design checks: a frame of layers is checked from the document when its work is done", async (ctx) => {
  await openCanvas(ctx);
  const begun = await ctx.call({
    cmd: "canvas",
    op: { op: "begin", name: "Save button", frame: { x: 0, y: 0, width: 240, height: 40 } },
  });
  assert.equal(begun.ok, true, JSON.stringify(begun));
  await ctx.call({ cmd: "canvas", op: { op: "progress", work: begun.work, status: "done" } });
  const checked = await ctx.call({ cmd: "canvas", op: { op: "checks", work: begun.work } });
  assert.equal(checked.ok, true, JSON.stringify(checked));
  assert.deepEqual(checked.findings.map((f) => f.rule), ["a11y.2"]);
  await ctx.win.waitFor({ role: "button", name: "1 design issue" });
});
