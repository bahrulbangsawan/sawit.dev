// agent-desktop, driving one Sawit window through the macOS accessibility tree.
//
// Every call names the window by its id, found from the pid of the Sawit this test started, so
// nothing here reads or touches another Sawit window (the person's own included). Actions are
// headless by default: a click is the element's accessibility "press", typing into a field is an
// accessibility value write, and neither moves the pointer nor needs the window in front. Key
// presses are real key events, posted to the Sawit process alone. Only `drag` (and anything with
// `--headed`) moves the real pointer, so tests that need it run with `--headed` only.

import { execFile } from "node:child_process";
import { appendFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";

import { until } from "./wait.js";

const run = promisify(execFile);

export const AGENT_DESKTOP = path.resolve(
  import.meta.dirname,
  "..",
  "node_modules",
  "agent-desktop",
  "bin",
  "agent-desktop",
);

/**
 * Buttons a test never presses: they change the person's machine (the Tools page sees the
 * Homebrew installs in /opt/homebrew whatever HOME is, and Install CLI writes to /usr/local/bin)
 * or delete things. A click on one is refused before agent-desktop sees it, as is a click on a
 * button with no name, which could be any of them.
 */
const NEVER_PRESS =
  /\b(install|installs|reinstall|upgrade|update|updates|remove|uninstall|delete|trash|sign in)\b/i;

export class DesktopError extends Error {
  constructor(message, envelope) {
    super(message);
    this.envelope = envelope;
  }
}

/** The agent-desktop CLI for one test, its calls logged to `logFile`. */
export class Desktop {
  constructor({ env, logFile, headed = false }) {
    this.env = env;
    this.logFile = logFile;
    this.headed = headed;
  }

  /** Runs one command; resolves with its `data`, or throws with its error envelope. */
  async call(args, { headed = false, timeoutMs = 60_000 } = {}) {
    const argv = headed ? ["--headed", ...args] : args;
    const started = Date.now();
    let stdout = "";
    let failure = null;
    try {
      ({ stdout } = await run(AGENT_DESKTOP, argv, {
        env: this.env,
        maxBuffer: 64 << 20,
        timeout: timeoutMs,
      }));
    } catch (e) {
      stdout = e.stdout || "";
      failure = e;
    }
    let envelope = null;
    try {
      envelope = JSON.parse(stdout);
    } catch {}
    const ms = Date.now() - started;
    const brief = envelope?.ok
      ? "ok"
      : `${envelope?.error?.code ?? "?"} ${envelope?.error?.message ?? failure?.message ?? ""}`;
    await appendFile(this.logFile, `${new Date().toISOString()} ${ms}ms agent-desktop ${argv.join(" ")} → ${brief}\n`);
    if (!envelope?.ok) {
      const error = envelope?.error;
      throw new DesktopError(
        `agent-desktop ${args[0]} failed: ${error?.code ?? "no envelope"} ${error?.message ?? failure?.message ?? ""}`,
        envelope,
      );
    }
    return envelope.data;
  }

  /**
   * The id (`w-…`) of the window process `pid` shows, once accessibility can reach it. A window
   * of a Sawit just started, on a Mac busy building, has been seen to stay out of reach for over
   * 20 s: hence the long deadline.
   */
  async windowOf(pid, { timeoutMs = 60_000 } = {}) {
    let seen = [];
    try {
      return await until(
        async () => {
          const data = await this.call(["list-windows"]);
          const windows = Array.isArray(data) ? data : (data.windows ?? []);
          seen = windows
            .filter((w) => /sawit/i.test(w.app_name ?? ""))
            .map((w) => `${w.id} pid ${w.pid} ${w.accessible ? "" : "not "}accessible`);
          return windows.find((w) => w.pid === pid && w.accessible)?.id ?? null;
        },
        { timeoutMs, what: `a window of pid ${pid}` },
      );
    } catch (e) {
      e.message += ` (Sawit windows seen: ${seen.join("; ") || "none"})`;
      throw e;
    }
  }

  /** The main display's size in points: `{ width, height }`. */
  async display() {
    const displays = await this.call(["list-displays"]);
    return (displays.find((d) => d.is_primary) ?? displays[0]).bounds;
  }

  /** A window of this test's app: see [`Window`]. */
  window(id) {
    return new Window(this, id);
  }
}

/** Roles whose value is checked or not: agent-desktop reports it as "true" or "false". */
const TOGGLES = new Set(["radiobutton", "switch", "checkbox", "menuitem", "togglebutton"]);
const asBool = (v) => (v === "true" ? true : v === "false" ? false : (v ?? null));

/**
 * A node of a snapshot, flattened: `{ role, name, value, description, ref, states, bounds,
 * depth, path }`, `path` being the names of its ancestors.
 */
function flatten(tree) {
  const out = [];
  const walk = (node, depth, trail) => {
    out.push({
      role: node.role,
      name: node.name ?? null,
      value: TOGGLES.has(node.role) ? asBool(node.value) : (node.value ?? null),
      description: node.description ?? null,
      ref: node.ref_id ?? null,
      states: node.states ?? [],
      bounds: node.bounds ?? null,
      depth,
      path: trail,
    });
    const next = node.name ? [...trail, node.name] : trail;
    for (const child of node.children ?? []) walk(child, depth + 1, next);
  };
  walk(tree, 0, []);
  return out;
}

/** Whether `node` matches `{ role, name, value, within }`: strings exactly, RegExps by test. */
function matches(node, { role, name, value, within } = {}) {
  // undefined: anything; null: nothing (a field with no name); a RegExp: tested; else equal.
  const test = (want, got) =>
    want === undefined ||
    (want === null
      ? got === null || got === undefined
      : want instanceof RegExp
        ? want.test(String(got ?? ""))
        : String(want) === String(got ?? ""));
  return (
    test(role, node.role) &&
    test(name, node.name) &&
    test(value, node.value) &&
    (within === undefined || node.path.some((n) => test(within, n)))
  );
}

const describe = (query) =>
  Object.entries(query)
    .filter(([, v]) => v !== undefined)
    .map(([k, v]) => `${k}=${v}`)
    .join(" ");

/** One window: snapshots of it, and actions on its elements found by role and name. */
export class Window {
  constructor(desktop, id) {
    this.desktop = desktop;
    this.id = id;
  }

  /**
   * Brings the window to the front and waits until macOS confirms it has the focus. The first
   * test of a run starts while another app (the terminal that started the suite) is in front,
   * and macOS may hold a new app back from taking it for a moment; keys sent before then are
   * refused ("the named window's exact focus could not be confirmed").
   */
  async takeFocus({ timeoutMs = 15_000 } = {}) {
    await until(
      async () => {
        try {
          await this.desktop.call(["focus-window", "--window-id", this.id]);
          return true;
        } catch {
          return false;
        }
      },
      { timeoutMs, intervalMs: 500, what: "the window to have the focus" },
    );
  }

  /**
   * Makes the window as large as the main display, so the lists' feet (the pager, the Show menu)
   * are on screen: agent-desktop presses only what is visible.
   */
  async fill() {
    const { width, height } = await this.desktop.display();
    const size = { width: Math.round(width), height: Math.round(height) };
    // The move and the resize land, but agent-desktop's check that they did can run out of time
    // while the window is still settling: what counts is the frame list-windows reports after.
    const lenient = (args) =>
      this.desktop.call(args).catch((e) => {
        if (e.envelope?.error?.disposition?.delivery === "not_delivered") throw e;
      });
    // One after the other: agent-desktop's command host takes one request at a time, and a
    // second one sent meanwhile expires waiting.
    await lenient(["move-window", "--window-id", this.id, "--x", "0", "--y", "0"]);
    await lenient(["resize-window", "--window-id", this.id, "--width", String(size.width), "--height", String(size.height)]);
    // macOS keeps a window clear of the menu bar and the Dock, so it may end up smaller than
    // asked: what matters is that it has stopped changing.
    let last = null;
    await until(
      async () => {
        const data = await this.desktop.call(["list-windows"]);
        const windows = Array.isArray(data) ? data : (data.windows ?? []);
        const now = JSON.stringify(windows.find((w) => w.id === this.id)?.bounds ?? null);
        const settled = now === last && now !== "null";
        last = now;
        return settled;
      },
      { timeoutMs: 10_000, intervalMs: 300, what: "the window to settle at its new size" },
    );
  }

  /** The whole tree, every node with its role and name (text the app exposes as labels too). */
  async snapshot({ bounds = false, surface } = {}) {
    const args = ["snapshot", "--window-id", this.id, "--compact", "--max-depth", "50"];
    if (bounds) args.push("--include-bounds");
    if (surface) args.push("--surface", surface);
    args.push("--timeout-ms", "10000");
    const data = await this.desktop.call(args);
    return { raw: data, nodes: flatten(data.tree) };
  }

  /** The nodes under the first node matching `query` now (not the node itself). */
  async subtree(query) {
    const { nodes } = await this.snapshot();
    const at = nodes.findIndex((n) => matches(n, query));
    if (at < 0) return [];
    const out = [];
    for (const n of nodes.slice(at + 1)) {
      if (n.depth <= nodes[at].depth) break;
      out.push(n);
    }
    return out;
  }

  /** The nodes matching `query` now. */
  async findAll(query, opts) {
    const { nodes } = await this.snapshot(opts);
    return nodes.filter((n) => matches(n, query));
  }

  /** Waits for a node matching `query` and resolves with it (the first, in tree order). */
  async waitFor(query, { timeoutMs = 15_000, bounds = false } = {}) {
    return until(
      async () => (await this.findAll(query, { bounds }))[0] ?? null,
      { timeoutMs, intervalMs: 250, what: describe(query) },
    );
  }

  /** Waits until no node matches `query`. */
  async waitForGone(query, { timeoutMs = 15_000 } = {}) {
    await until(async () => (await this.findAll(query)).length === 0, {
      timeoutMs,
      intervalMs: 250,
      what: `no ${describe(query)}`,
    });
  }

  /**
   * Presses the element matching `query` through its accessibility action (no pointer, no
   * focus change). Refuses buttons that install, update or remove anything, and nameless ones.
   */
  async click(query, { timeoutMs = 15_000 } = {}) {
    const node = await this.waitFor(query, { timeoutMs });
    guard(node);
    if (!node.ref) throw new Error(`${describe(query)} has no ref to act on (${node.role})`);
    return this.desktop.call(["click", node.ref]);
  }

  /**
   * Opens the dropdown menu of the button matching `query` (a "Show 8", a device or a package
   * manager menu) the keyboard's way: focus on the button, then Return. A dropdown button of
   * gpui-component opens on a mouse press or on Return, and has no accessibility press action
   * (its own click handler is the menu's), so `click` can't open it.
   */
  async openMenu(query, { timeoutMs = 15_000 } = {}) {
    const node = await this.waitFor(query, { timeoutMs });
    guard(node);
    await this.desktop.call(["focus", node.ref]);
    await this.press("return");
  }

  /** Gives the keyboard focus to the element matching `query` (its accessibility focus action). */
  async focus(query, { timeoutMs = 15_000 } = {}) {
    const node = await this.waitFor(query, { timeoutMs });
    return this.desktop.call(["focus", node.ref]);
  }

  /** Writes `text` as the value of the field matching `query` (an accessibility value write). */
  async setValue(query, text, { timeoutMs = 15_000 } = {}) {
    const node = await this.waitFor(query, { timeoutMs });
    return this.desktop.call(["set-value", node.ref, text]);
  }

  /**
   * Presses a key combo (`return`, `cmd+s`, `e`). The key events go to this window's process
   * only, which hands them to the element with the keyboard focus, without bringing the window
   * forward.
   */
  async press(combo) {
    return this.desktop.call(["press", combo, "--window-id", this.id]);
  }

  /**
   * `press` for a key meant for the web page in the browser tab. The keyboard focus is then the
   * WKWebView's content, which agent-desktop can't confirm as this window's (STALE_REF), so the
   * window (already in front) is focused by its id and the key sent again.
   */
  async pressInPage(combo) {
    try {
      return await this.press(combo);
    } catch (e) {
      if (e.envelope?.error?.code !== "STALE_REF") throw e;
      await this.desktop.call(["focus-window", "--window-id", this.id]);
      return this.press(combo);
    }
  }

  /** Types `text` as key presses, one combo each, in one agent-desktop batch. */
  async typeKeys(text) {
    const entries = [...text].map((ch) => ({
      command: "press",
      args: { combo: keyFor(ch), window_id: this.id },
    }));
    return this.desktop.call(["batch", JSON.stringify(entries), "--stop-on-error"]);
  }

  /**
   * Drags with the real pointer from one screen point to another. This moves the person's
   * cursor, so only tests run with `--headed` may call it.
   */
  async drag(from, to, { dropDelayMs = 300 } = {}) {
    if (!this.desktop.headed) {
      throw new Error("drag moves the real pointer: run the suite with --headed for this test");
    }
    return this.desktop.call(
      [
        "drag",
        "--from-xy",
        `${Math.round(from.x)},${Math.round(from.y)}`,
        "--to-xy",
        `${Math.round(to.x)},${Math.round(to.y)}`,
        "--drop-delay",
        String(dropDelayMs),
      ],
      { headed: true },
    );
  }

  /**
   * Right-clicks the element matching `query` with the real pointer, at the middle of its
   * accessibility bounds. GPUI's context menus open on a mouse press only (macOS's accessibility
   * "show menu" doesn't reach them, and agent-desktop's `right-click` on a ref gives up when it
   * finds no such action), so this is a coordinate click, but at a point read from the element.
   * Needs `--headed`.
   */
  async rightClick(query) {
    if (!this.desktop.headed) {
      throw new Error("a right-click moves the real pointer: run the suite with --headed for this test");
    }
    const node = await this.waitFor(query, { bounds: true });
    const { x, y, width, height } = node.bounds;
    const at = `${Math.round(x + width / 2)},${Math.round(y + height / 2)}`;
    return this.desktop.call(["mouse-click", "--button", "right", "--xy", at], { headed: true });
  }

  /** A PNG of the window (needs Screen Recording for the app that runs the suite). */
  async screenshot(file) {
    return this.desktop.call(["screenshot", "--window-id", this.id, file]);
  }
}

/** The window's own buttons, beside its content: a click on "Close" must never close the window. */
const WINDOW_CONTROLS = new Set(["Close", "Minimize", "Full Screen", "Zoom"]);

function guard(node) {
  const name = node.name ?? "";
  if (node.depth === 1 && WINDOW_CONTROLS.has(name)) {
    throw new Error(`refusing to press the window's own “${name}” button: scope the query with within`);
  }
  if (node.role === "button" && !name) {
    throw new Error("refusing to press a button with no accessible name: it could be anything");
  }
  // A name ending in an ellipsis opens a sheet or dialog that asks first, as on any Mac
  // ("Install Figma…" opens the install sheet): pressing it changes nothing yet.
  if (NEVER_PRESS.test(name) && !name.endsWith("…")) {
    throw new Error(`refusing to press “${name}”: the tests never install, update or remove`);
  }
}

/** The `press` combo that types `ch` (on a US layout, as `minus` and `slash` assume too). */
function keyFor(ch) {
  const named = {
    " ": "space",
    "-": "minus",
    ".": "period",
    "/": "slash",
    ",": "comma",
    "&": "shift+7",
    "\n": "return",
  };
  if (named[ch]) return named[ch];
  if (/^[a-z0-9]$/.test(ch)) return ch;
  if (/^[A-Z]$/.test(ch)) return ch; // agent-desktop adds Shift to a bare capital
  throw new Error(`no key combo for ${JSON.stringify(ch)}`);
}
