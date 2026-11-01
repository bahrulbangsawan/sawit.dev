// One Sawit process for one test: started in the temp world with nothing of the person's
// environment, found again by its pid, and killed by that pid at the end.

import { execFile, spawn } from "node:child_process";
import { createWriteStream, existsSync } from "node:fs";
import path from "node:path";
import { promisify } from "node:util";

import { send } from "./control.js";
import { SYSTEM_PATH } from "./fixture.js";
import { until } from "./wait.js";

const run = promisify(execFile);

export const REPO = path.resolve(import.meta.dirname, "..", "..");
export const BINARY = process.env.SAWIT_E2E_BINARY || path.join(REPO, "target", "debug", "sawit");

/**
 * The environment Sawit starts with: `env -i` plus these. HOME and SAWIT_HOME point into the temp
 * world, so the app reads and writes no data, agent configs (the MCP page edits them under HOME)
 * or saved logins of the person's, and its shells start in that home too.
 */
export function appEnv(world) {
  return {
    HOME: world.home,
    SAWIT_HOME: world.sawitHome,
    TMPDIR: `${world.tmp}/`,
    PATH: SYSTEM_PATH,
    USER: process.env.USER || "e2e",
    LOGNAME: process.env.USER || "e2e",
    SHELL: "/bin/zsh",
    LANG: "en_US.UTF-8",
    // Errors and warnings in the app log the test keeps on failure; debug is far too chatty.
    RUST_LOG: "warn",
    RUST_BACKTRACE: "1",
    // Marks the process as this run's, so run.sh can find one the harness failed to kill.
    SAWIT_E2E_RUN: process.env.SAWIT_E2E_RUN_ID || "adhoc",
  };
}

/** The pids of the Sawits this process started that are still running. */
const LIVE = new Set();

// Interrupted (Ctrl-C, or the runner's timeout), a test's teardown doesn't get to run: the Sawits
// it started go down with it all the same.
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.once(signal, () => {
    for (const pid of LIVE) {
      try {
        process.kill(pid, "SIGKILL");
      } catch {}
    }
    process.exit(130);
  });
}

/**
 * Starts `sawit --foreground <project>` (`--foreground` makes this process the app, rather than
 * handing the folder to a Sawit already running, which may be the person's own). Resolves once its
 * own control socket answers with a window: `{ pid, socket, call, kill, log }`.
 */
export async function launch(world, { args = [world.project], timeoutMs = 60_000 } = {}) {
  if (!existsSync(BINARY)) {
    throw new Error(`${BINARY} is missing: build it with \`cargo build -p sawit\``);
  }
  const logPath = path.join(world.artifacts, "app.log");
  const log = createWriteStream(logPath);
  const env = appEnv(world);
  const envArgs = ["-i", ...Object.entries(env).map(([k, v]) => `${k}=${v}`)];
  const child = spawn("/usr/bin/env", [...envArgs, BINARY, "--foreground", ...args], {
    cwd: world.project,
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.pipe(log);
  child.stderr.pipe(log);
  let exited = null;
  child.on("exit", (code, signal) => {
    exited = { code, signal };
    LIVE.delete(child.pid);
  });
  // `env` execs Sawit in place, so the child's pid is Sawit's.
  const pid = child.pid;
  LIVE.add(pid);

  const app = {
    pid,
    child,
    logPath,
    socket: null,
    exited: () => exited,
    call: (request, opts) => send(app.socket, request, opts),
    kill: () => kill(child, () => exited),
  };
  try {
    app.socket = await until(
      async () => {
        if (exited) throw new Error(`Sawit exited before it listened (${JSON.stringify(exited)})`);
        return socketOf(pid, world);
      },
      { timeoutMs, what: "Sawit's control socket" },
    );
    await until(
      async () => {
        const state = await app.call({ cmd: "state" }, { timeoutMs: 3_000 }).catch(() => null);
        return state?.windows?.length ? state : null;
      },
      { timeoutMs, what: "a Sawit window" },
    );
  } catch (e) {
    await app.kill();
    throw e;
  }
  return app;
}

/**
 * The Unix socket process `pid` listens on, from `lsof`. Only this process's own sockets are
 * looked at, and only one inside this test's Sawit folder (or the `/tmp` fallback Sawit uses for
 * a long one), so a request never reaches the person's Sawit.
 */
export async function socketOf(pid, world) {
  let stdout = "";
  try {
    ({ stdout } = await run("/usr/sbin/lsof", ["-nP", "-a", "-U", "-p", String(pid), "-F", "n"]));
  } catch (e) {
    // lsof exits 1 when it finds nothing yet.
    stdout = e.stdout || "";
  }
  const names = stdout
    .split("\n")
    .filter((l) => l.startsWith("n"))
    .map((l) => l.slice(1));
  return (
    names.find((n) => n.startsWith(world.sawitHome) && n.endsWith(".sock")) ||
    names.find((n) => /^\/tmp\/sawit-.*\.sock$/.test(n) || /^\/private\/tmp\/sawit-.*\.sock$/.test(n)) ||
    null
  );
}

/** Ends the process: SIGTERM, then SIGKILL if it is still there after 3 s. */
async function kill(child, exited) {
  if (exited()) return;
  const gone = new Promise((resolve) => child.once("exit", resolve));
  try {
    process.kill(child.pid, "SIGTERM");
  } catch {
    return;
  }
  const timer = new Promise((resolve) => setTimeout(() => resolve("timeout"), 3_000));
  if ((await Promise.race([gone, timer])) === "timeout" && !exited()) {
    try {
      process.kill(child.pid, "SIGKILL");
    } catch {}
    await gone;
  }
}
