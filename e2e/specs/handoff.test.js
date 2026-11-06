// Send to Agent for artboards: the bar over a selected frame lists the coding agents, Copy Prompt
// and Reveal Handoff Folder; Copy Prompt writes the handoff folder; an agent opens in a new
// terminal tab whose first message is the handoff's prompt. (Whether the agent then builds a page
// that matches design.png needs a signed-in agent and the user: not checked here.)

import assert from "node:assert/strict";
import { chmod, mkdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";

import { sawitTest } from "../lib/harness.js";
import { until } from "../lib/wait.js";
import { openCanvas, terminalText, windowState } from "../lib/sawit.js";

async function command(ctx, command, params) {
  const done = await ctx.call({ cmd: "canvas", op: { op: "command", command, params } });
  assert.equal(done.ok, true, JSON.stringify(done));
  return done.result;
}

/** A phone artboard, selected. */
async function drawArtboard(ctx) {
  const frame = await command(ctx, "frame.create", {
    x: 0, y: 0, width: 393, height: 600, name: "Settings", props: { fill: "#ffffff" },
  });
  await command(ctx, "text.create", {
    x: 24, y: 24, text: "Settings", parent: frame.id, name: "Title", props: { fontSize: 24, fill: "#111827" },
  });
  await command(ctx, "select.none", {});
  await command(ctx, "select.add", { ids: [frame.id] });
  return frame.id;
}

const exists = async (file) => (await stat(file).catch(() => null))?.isFile() ?? false;

sawitTest("handoff: Copy Prompt on an artboard writes its handoff folder", async (ctx) => {
  const { win, world } = ctx;
  await openCanvas(ctx);
  await drawArtboard(ctx);
  await win.openMenu({ role: "button", name: "Send to Agent…" });
  await win.waitFor({ role: "menuitem", name: "Reveal Handoff Folder" });
  await win.click({ role: "menuitem", name: "Copy Prompt" });
  const out = path.join(world.project, ".nyawit", "handoff", "settings");
  for (const file of ["design.png", "settings.html", "Settings.tsx", "Settings.module.css", "spec.md", "prompt.md"]) {
    await until(() => exists(path.join(out, file)), { what: `${file} in ${out}` });
  }
});

sawitTest("handoff: an agent opens in a new terminal tab with the handoff's prompt", async (ctx) => {
  const { win, world } = ctx;
  // A stand-in Claude Code in the temp home, found where the Dev Tools page looks: it says what it
  // was started with, and builds nothing.
  const bin = path.join(world.home, ".local", "bin");
  await mkdir(bin, { recursive: true });
  const fake = path.join(bin, "claude");
  await writeFile(fake, '#!/bin/sh\necho "stand-in claude started with $(printf %s "$1" | head -c 40)"\n');
  await chmod(fake, 0o755);
  await openCanvas(ctx);
  await drawArtboard(ctx);
  const before = (await windowState(ctx)).terminals?.length ?? 0;
  await win.openMenu({ role: "button", name: "Send to Agent…" });
  const items = await win.findAll({ role: "menuitem" });
  const agent = items.map((i) => i.name).find((n) => ["Claude Code", "Codex", "Gemini CLI", "OpenCode"].includes(n));
  assert.ok(agent, `no coding agent found on this Mac: ${items.map((i) => i.name)}`);
  await win.click({ role: "menuitem", name: agent });
  const tab = await until(async () => {
    const terminals = (await windowState(ctx)).terminals ?? [];
    return terminals.length > before ? terminals.length - 1 : null;
  }, { what: "a new terminal tab" });
  const text = await until(async () => {
    const t = await terminalText(ctx, { tab, lines: 80 });
    return t.includes(".nyawit/handoff/settings/prompt.md") ? t : null;
  }, { what: "the agent started with the prompt file" });
  assert.match(text, /"\$\(cat -- .*prompt\.md\)"/);
});
