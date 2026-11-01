// A throwaway world for one test: a home folder, a Sawit data folder and a project, all under one
// temp folder that teardown removes. Nothing here reads or writes the person's own files.

import { execFile } from "node:child_process";
import { mkdtemp, mkdir, writeFile, realpath } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);

/** The PATH Sawit gets: the system's only, so the Tools page sees no installs of the person's. */
export const SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin";

/**
 * Makes the temp folders: `{ root, home, sawitHome, project, tmp, artifacts }`. The root is
 * resolved through its links (`/var` is `/private/var`), as Sawit resolves the paths it shows.
 */
export async function makeWorld() {
  // Named after the run, so run.sh can remove the worlds of tests it interrupted, and short:
  // Sawit's control socket goes in it, and a socket's path has to fit in 104 bytes on macOS.
  const run = process.env.SAWIT_E2E_RUN_ID || "adhoc";
  const root = await realpath(await mkdtemp(path.join(os.tmpdir(), `se-${run}-`)));
  const world = {
    root,
    home: path.join(root, "home"),
    // Short on purpose: the control socket goes inside it, and socket paths are limited to 104
    // bytes on macOS.
    sawitHome: path.join(root, "sh"),
    project: path.join(root, "home", "fixture-app"),
    tmp: path.join(root, "tmp"),
    artifacts: path.join(root, "artifacts"),
  };
  for (const dir of [world.home, world.sawitHome, world.project, world.tmp, world.artifacts]) {
    await mkdir(dir, { recursive: true, mode: 0o700 });
  }
  // The shell in Sawit's terminal reads this home's startup files, not the person's: a plain
  // prompt keeps terminal output easy to read back.
  await writeFile(
    path.join(world.home, ".zshrc"),
    "PROMPT='e2e%# '\nunsetopt BEEP\n",
  );
  // An agent's skills folder, so the Skills page has an agent to offer its packs to (it lists
  // none, and its sheets don't open, on a Mac with no agent). It is empty and in the temp home.
  await mkdir(path.join(world.home, ".agents", "skills"), { recursive: true });
  // Sawit asks the login shell for PATH on its first launch; a cached answer skips that, and pins
  // PATH to the system's.
  await mkdir(path.join(world.sawitHome, "cache"), { recursive: true });
  await writeFile(path.join(world.sawitHome, "cache", "login-path"), SYSTEM_PATH);
  return world;
}

/** The files of the fixture project, by path relative to it. */
export const PROJECT_FILES = {
  "README.md": "# Fixture app\n\nA small project Sawit's end-to-end tests open.\n",
  "notes.txt": "first line\nsecond line\n",
  "src/main.js": 'export function greet(name) {\n  return `hello ${name}`;\n}\n',
  "site/index.html":
    '<!doctype html>\n<html><head><meta charset="utf-8"><title>Fixture Site</title></head>\n' +
    '<body><h1 id="title">Hello from the fixture site</h1>\n' +
    '<p id="width"></p>\n' +
    "<script>document.getElementById('width').textContent = 'width ' + innerWidth;</script>\n" +
    "</body></html>\n",
  ".nyawit/.keep": "",
};

/**
 * Writes the fixture project into `world.project`, with a git repository of one commit. Git runs
 * with the temp home and no system config, so the person's git config and hooks play no part.
 */
export async function makeProject(world) {
  for (const [rel, text] of Object.entries(PROJECT_FILES)) {
    const file = path.join(world.project, rel);
    await mkdir(path.dirname(file), { recursive: true });
    await writeFile(file, text);
  }
  const env = {
    HOME: world.home,
    PATH: SYSTEM_PATH,
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_AUTHOR_NAME: "Sawit E2E",
    GIT_AUTHOR_EMAIL: "e2e@sawit.invalid",
    GIT_COMMITTER_NAME: "Sawit E2E",
    GIT_COMMITTER_EMAIL: "e2e@sawit.invalid",
  };
  const git = (...args) => run("/usr/bin/git", args, { cwd: world.project, env });
  await git("init", "-q", "-b", "main");
  await git("add", "-A");
  await git("commit", "-q", "-m", "Fixture project");
  return world.project;
}

/**
 * Serves the fixture project's `site/` folder on a free loopback port. Resolves with
 * `{ url, close }`; `close` ends every connection, so no server outlives the test.
 */
export async function serveSite(world) {
  const { readFile } = await import("node:fs/promises");
  const dir = path.join(world.project, "site");
  const sockets = new Set();
  const server = http.createServer(async (req, res) => {
    const rel = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const file = path.join(dir, rel.endsWith("/") ? `${rel}index.html` : rel);
    if (!file.startsWith(dir)) {
      res.writeHead(403).end();
      return;
    }
    try {
      const body = await readFile(file);
      const type = file.endsWith(".html") ? "text/html; charset=utf-8" : "application/octet-stream";
      res.writeHead(200, { "content-type": type, "cache-control": "no-store" }).end(body);
    } catch {
      res.writeHead(404).end("not found");
    }
  });
  server.on("connection", (s) => {
    sockets.add(s);
    s.on("close", () => sockets.delete(s));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  return {
    url: `http://127.0.0.1:${port}/`,
    port,
    close: () =>
      new Promise((resolve) => {
        for (const s of sockets) s.destroy();
        server.close(() => resolve());
      }),
  };
}
