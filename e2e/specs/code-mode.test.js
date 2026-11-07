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
