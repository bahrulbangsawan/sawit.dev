// Code mode: an agent whose sawit-mcp lists only search_api and run drives the running app with
// one script: it runs a command in a terminal, reads that terminal's output by the tab and pane
// terminal_run returned, and reads app_state, and gets back only what the script returns.
//
// sawit-mcp is the app's own binary started as `sawit-mcp` (a symlink), in the test's world, the
// way an AI client starts it, with SAWIT_MCP_TOOLS=code in its environment.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { symlink } from "node:fs/promises";
import path from "node:path";
import { createInterface } from "node:readline";

import { BINARY, appEnv } from "../lib/app.js";
import { sawitTest } from "../lib/harness.js";

/** A minimal MCP client over the stdio of `sawit-mcp`: one request at a time, by id. */
async function mcpClient(world, env) {
  const program = path.join(world.tmp, "sawit-mcp");
  await symlink(BINARY, program);
  const child = spawn(program, [], {
    cwd: world.project,
    env: { ...appEnv(world), ...env },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const waiting = new Map();
  let next = 1;
  createInterface({ input: child.stdout }).on("line", (line) => {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      return;
    }
    const done = waiting.get(message.id);
    if (done) {
      waiting.delete(message.id);
      done(message);
    }
  });
  const request = (method, params) =>
    new Promise((resolve, reject) => {
      const id = next++;
      const timer = setTimeout(() => reject(new Error(`${method} got no answer`)), 60_000);
      waiting.set(id, (m) => {
        clearTimeout(timer);
        m.error ? reject(new Error(JSON.stringify(m.error))) : resolve(m.result);
      });
      child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  const init = await request("initialize", {
    protocolVersion: "2025-06-18",
    capabilities: {},
    clientInfo: { name: "e2e-code-mode", version: "1.0.0" },
  });
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`);
  return { init, request, close: () => child.kill() };
}

/** The text of a tools/call answer. */
const text = (result) =>
  result.content
    .filter((c) => c.type === "text")
    .map((c) => c.text)
    .join("\n");

sawitTest("code mode: one script runs a command, reads its terminal and the app's state", async (ctx) => {
  const client = await mcpClient(ctx.world, { SAWIT_MCP_TOOLS: "code" });
  try {
    assert.match(client.init.instructions, /search_api/);
    const { tools } = await client.request("tools/list", {});
    assert.deepEqual(
      tools.map((t) => t.name).sort(),
      ["run", "search_api"],
      "code mode lists only its two tools",
    );

    const found = await client.request("tools/call", {
      name: "search_api",
      arguments: { query: "terminal read" },
    });
    assert.match(text(found), /terminal_read\(args/);

    const script = `
      const ran = sawit.terminal_run({ command: "echo code-mode-ok", wait_secs: 1 });
      const [, tab, pane] = ran.match(/Terminal (\\d+), pane (\\d+)/).map(Number);
      let out = "";
      for (let i = 0; i < 20 && !/^code-mode-ok$/m.test(out); i++) {
        out = sawit.terminal_read({ tab, pane, lines: 20 });
      }
      const state = sawit.app_state({});
      return { tab, pane, ok: /^code-mode-ok$/m.test(out), windows: state.windows.length };
    `;
    const result = await client.request("tools/call", { name: "run", arguments: { code: script } });
    assert.ok(!result.isError, text(result));
    const answer = JSON.parse(text(result));
    assert.equal(answer.ok, true, `the terminal printed the line: ${text(result)}`);
    assert.ok(answer.windows >= 1);

    // The old names still answer while unlisted.
    const state = await client.request("tools/call", { name: "app_state", arguments: {} });
    assert.ok(!state.isError, text(state));
  } finally {
    client.close();
  }
});

sawitTest("terminal reuse: fifty commands from one agent leave one or two tabs", async (ctx) => {
  const client = await mcpClient(ctx.world, { SAWIT_MCP_TOOLS: "code" });
  try {
    const project = JSON.stringify(ctx.world.project);
    const script = `
      const before = sawit.app_state({}).windows[0].terminals.length;
      let last;
      for (let i = 0; i < 50; i++) {
        last = sawit.terminal_run({ command: "echo step-" + i, cwd: ${project}, wait_secs: 0.2 });
      }
      const run = Number(last.match(/\\(run (\\d+)/)[1]);
      let out = "";
      for (let i = 0; i < 30 && !/^step-49$/m.test(out); i++) out = sawit.terminal_read({ run, lines: 20 });
      // A one-off command in a tab of its own, which closes once it is done.
      const once = sawit.terminal_run({ command: "echo one-off", new_tab: true, close_when_done: true, wait_secs: 3 });
      const onceRun = Number(once.match(/\\(run (\\d+)/)[1]);
      // Scripts have no timers: wait by the clock between looks, for up to 20 seconds.
      let after = 0;
      const until = Date.now() + 20_000;
      while (Date.now() < until) {
        after = sawit.app_state({}).windows[0].terminals.length;
        if (after <= before + 1) break;
        const next = Date.now() + 500;
        while (Date.now() < next) {}
      }
      return { before, after, ok: /^step-49$/m.test(out), once: sawit.terminal_read({ run: onceRun, lines: 20 }) };
    `;
    const result = await client.request("tools/call", {
      name: "run",
      arguments: { code: script, timeout_secs: 300 },
    });
    assert.ok(!result.isError, text(result));
    const answer = JSON.parse(text(result));
    assert.equal(answer.ok, true, "the last command's output reads back by its run");
    assert.ok(answer.after <= answer.before + 1, `tabs before ${answer.before}, after ${answer.after}`);
    assert.match(answer.once, /one-off/);
  } finally {
    client.close();
  }
});
