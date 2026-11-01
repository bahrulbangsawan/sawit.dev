// The Mac the tests run on: whether someone is using it, and which app they had in front.
//
// A run brings Sawit windows to the front and may send real input, which would land in whatever
// the person at the Mac is typing. So a run waits until the keyboard and mouse have been idle a
// while, and hands the front back to the app that had it.

import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);

/** Seconds since the last keyboard or mouse event, from the HID system. */
export async function idleSeconds() {
  const { stdout } = await run("/usr/sbin/ioreg", ["-c", "IOHIDSystem", "-d", "4"]);
  const match = stdout.match(/"HIDIdleTime"\s*=\s*(\d+)/);
  if (!match) throw new Error("ioreg reported no HIDIdleTime");
  return Number(match[1]) / 1e9;
}

/**
 * Waits until nobody has touched the keyboard or mouse for `seconds`. Gives up after `timeoutS`
 * (0: never), so an unattended run does not hang forever on a Mac in use.
 */
export async function waitForIdle(seconds, { timeoutS = 0, log = () => {} } = {}) {
  const started = Date.now();
  let told = false;
  for (;;) {
    const idle = await idleSeconds();
    if (idle >= seconds) return idle;
    if (!told) {
      log(`waiting for the Mac to be idle ${seconds}s (idle ${idle.toFixed(0)}s now)…`);
      told = true;
    }
    if (timeoutS && (Date.now() - started) / 1000 > timeoutS) {
      throw new Error(`the Mac was not idle for ${seconds}s within ${timeoutS}s`);
    }
    // Waiting out the rest of the idle period at once, not polling: input resets the clock.
    await new Promise((r) => setTimeout(r, Math.max(1, seconds - idle) * 1000));
  }
}

// JXA through the Objective-C bridge, not Apple Events: reading and activating the front app
// needs no Automation permission this way.
const FRONT_APP = `ObjC.import("AppKit");
const a = $.NSWorkspace.sharedWorkspace.frontmostApplication;
JSON.stringify({ pid: a.processIdentifier, bundle: a.bundleIdentifier.js || null, name: a.localizedName.js || null });`;

/** The app in front now: `{ pid, bundle, name }`. */
export async function frontApp() {
  const { stdout } = await run("/usr/bin/osascript", ["-l", "JavaScript", "-e", FRONT_APP]);
  return JSON.parse(stdout.trim());
}

/** Where the pointer is, in global top-left coordinates (CGEvent's). */
export async function pointer() {
  const script = `ObjC.import("AppKit");
const p = $.NSEvent.mouseLocation;
const h = $.NSScreen.screens.objectAtIndex(0).frame.size.height;
JSON.stringify({ x: p.x, y: h - p.y });`;
  const { stdout } = await run("/usr/bin/osascript", ["-l", "JavaScript", "-e", script]);
  return JSON.parse(stdout.trim());
}

/** Puts the pointer back at `at` (from `pointer`), without a click. */
export async function movePointer(at) {
  const script = `ObjC.import("CoreGraphics");
$.CGWarpMouseCursorPosition({ x: ${Number(at.x)}, y: ${Number(at.y)} });`;
  try {
    await run("/usr/bin/osascript", ["-l", "JavaScript", "-e", script]);
  } catch {
    // Best effort: the pointer stays where the test left it.
  }
}

/** Brings the app with this pid back to the front, if it still runs. */
export async function activate(pid) {
  const script = `ObjC.import("AppKit");
const a = $.NSRunningApplication.runningApplicationWithProcessIdentifier(${Number(pid)});
a ? String(a.activateWithOptions(0)) : "gone";`;
  try {
    await run("/usr/bin/osascript", ["-l", "JavaScript", "-e", script]);
  } catch {
    // Best effort: the person's app may have quit meanwhile.
  }
}
