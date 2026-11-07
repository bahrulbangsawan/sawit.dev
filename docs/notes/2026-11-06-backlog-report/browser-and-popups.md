# Backlog report, part 3: browser, tabs and popups (items 16, 6, 10, 15, and two extras)

Back to the [summary](../2026-11-06-backlog-report.md).

Screenshots named here are under `sawit-wt/browser/e2e/artifacts/` (git-ignored, not in the repo).

---

## Item 16: device preview inflated small text

**Wrong:** at iPhone 15 size, text under ~20 px rendered at 20.46 px with its old line-height, so
lines overlapped. `devicePixelRatio` read 0.88.

**Cause (confirmed):** `viewport.rs` set `WKWebView.pageZoom` (wry `zoom`) to the fit scale.
WebKit clamps text to 9 device px, which is 20.46 CSS px at 0.44×. No other pageZoom,
magnification, minimum font size or DPR override existed in the browser.

**Built (`2a96253`, exact height `53053ff`):**
- The view is scaled with the canvas's `Engine::set_view_scale` (shared code in
  `engine/web_frame.rs`, not a copy), plus a new `clear_view_scale` that Responsive and full-size
  devices use to go back to the normal layout. pageZoom stays 1. It is only a fallback where WebKit
  lacks `_setViewScale:`.
- Each device profile has a `dpr` (iPhone 15: 3, Pixel 8: 2.625, iPads: 2), applied with the
  private `_setOverrideDeviceScaleFactor:` (checked with `respondsToSelector`). **It works.**
- Exact size: `fit` picks whole-point width and height that both land in `[w, w + 0.5)` at one
  scale, giving up under 3 % of scale. Before `53053ff`, iPhone 15 at 0.4908 measured 851 high.

**Measured** (control socket, 1040 × 431 page area, before → after):

| Device | Before | After |
|---|---|---|
| iPhone 15 | fs and root 20.58 px, dpr 0.87 | 12 px / 16 px, root 16 px, 393 × 852, dpr 3 |
| Pixel 8 | 22.08 px | 12 px, dpr 2.625 |
| iPad mini | 27.23 px | 12 px, dpr 2 |
| Desktop | 21.60 px | 12 px, dpr 2 |
| Responsive | — | unchanged |

**Checked:** e2e `device: iPhone 15 lays out at 393 × 852 …` at three window sizes (scales 0.491,
0.407, 0.275), then iPad mini and landscape, and element screenshot size; `device: Desktop and
Responsive keep …`. Both pass (browser-e2e-4 and -9). The headed `annotate-device` spec clicks
`h1#title` at its computed screen position at 49 % and 37 %, and the popover names `h1#title`, so
clicks and picks map right. An agent click on a link opens it; the caption's percent is right.
Screenshots: `before-16/` and `after-16/` (iPhone 15, Pixel 8, iPad mini, Desktop, responsive).

**Decision / deviation:** Laptop, Desktop and custom sizes, while shrunk, keep the screen's ratio
(2 on Retina), with no override at full size.

**Not checked:** the user's own browser font-size settings were not tested separately.

---

## Extra: the device-mode × back to Responsive

**Asked by the user.** **Built in `2a96253`:** `browser/device.rs` (the device control moved out of
`toolbar.rs`, which was at the line limit). While the page isn't Responsive, an × sits beside the
device button, with the tooltip and accessible name "Back to Responsive". **Checked:** a GPUI unit
test and the `device: Desktop and Responsive keep …` e2e.

## Extra: a visible loading state for URLs

**Asked by the user.** **Built (`b9e0c4c`):** `browser/progress.rs`, a 2 pt bar along the toolbar's
foot (over its border, so the web view never covers it), from WebKit's `estimatedProgress`. It is
polled every 100 ms only while loading, never goes back within a load and never shows empty (8 %).
A shared `Progress` component, label "Loading the page", eased between readings (instant under
reduced motion). Reload already turned into Stop while loading. `app_state.browser.progress`.
**Checked:** e2e `loading: a slow page shows progress and Stop …` (fixture `/slow?ms=6000`) passes.

---

## Item 6: local dev servers in the browser

**Built (`58d5139`):** a "Local Servers" button after the address. Its popover lists each port of
each dev server (`localhost:<port>`, the command, the project folder), scanned only when it opens
(`sawit_core::ports::local_servers`; databases, Docker and non-dev apps left out). Choosing one
opens `http://localhost:<port>/` in the tab. After item 10, ⌘-click opens it in a new tab behind
the one on show.

**MCP:** none added; agents already have `list_ports`.

**Checked:** unit test of `local_servers`; e2e `servers: the address bar's list shows the dev
servers running, and opens one` (two node servers, both listed, the second opened) passes.

**Note:** `b9e0c4c` and `58d5139` were staged hunk by hunk from one working tree. The final tree was
built and tested, but these two intermediate commits were not each built on their own.

---

## Item 10: tabs in the browser and the canvas

**Built (`1605744` browser, `326d8c8` canvas):**
- **Browser:** `BrowserTabs`, a strip above the toolbar (titles cut with a tooltip, close ×, New
  Tab +, drag to reorder). Each tab is a `BrowserView` with its own address, history, device size,
  notes and panels. ⌘T, ⌘W, ⌘1…⌘8 and ⌘9 (last) are bound only while the browser has the keyboard.
- **Suspension:** only the tab on show keeps its `WKWebView`. A tab off screen for 60 s drops it
  and keeps the address and WebKit's `interactionState` (back/forward list, scroll, form state),
  then rebuilds it with that history when shown.
- **Agents on a tab not on show:** `browser_*` take `tab` (id, title or address). The tab is woken
  hidden, never key and without a keyboard host. `browser_tabs` (list/open/close/activate);
  `app_state.browser.tabs`.
- **Restore:** `.nyawit/browser-tabs.json` (address, title, device size, history), written 1 s after
  a change. ⌘W on the last tab forgets them.
- **Canvas:** canvases are `.sawit` files in `.nyawit/`; `canvas.sawit` stays the first. New Canvas,
  Rename Canvas… and Delete Canvas… (asks; the file goes to the Trash). The project's canvas can't
  be renamed or deleted. Open canvases are kept in `.nyawit/canvas-tabs.json`. `canvas_tabs`
  (list/open/close/activate/rename), `tab` on every `canvas_*` tool, `app_state.canvases`.

**Decisions:**
- Browser tabs live inside the project's one Browser tab; canvases use workspace tabs, because a
  canvas file already gets its own workspace tab.
- ⌘T in the browser opens a browser tab, which changes what ⌘T did while the page had the keyboard.
- Suspend after 60 s off screen, not at once, so switching between two tabs stays instant.
- An agent can't close a tab with unsent notes, and deleting a canvas is the user's only.

**Checked:** e2e `tabs.test.js` (three browser tabs opened, switched by click and ⌘1, closed and
restored after a restart; an agent reading and clicking on a background tab while the user types
in a terminal; two canvases opened, switched, closed and restored) and `tabs-memory` (see item 7).
The tabs specs first crashed the app: wry's `url()` unwrapped on a tab with no address yet. Fixed in
`57f78df`; the tabs specs then pass (fix-ui track). `d779962` makes the spec ask again after an empty
state reply.

**Note:** `1605744` builds and its app and core tests pass, but two sawit-mcp tests (tool list and
count) pass only from `326d8c8`. History was not rewritten.

---

## Item 15: popovers, dropdowns and menus that spilled out

### Case A: the annotate "Send the notes to" menu (GPUI)

**Built:** the menu is in gpui-component, not gpui-base, so gpui-component 0.8.1 was vendored
unchanged (`318436b`; ~4 MB of source, see `SAWIT_PATCH.md`) and patched (`b5284d6`):
- menu labels stay on one line, end in an ellipsis and keep their right padding;
- the shortcut is never squeezed, and a cut label gets a tooltip with the full text;
- the menu fits its content up to a soft 500 px and never past the window minus 8 px;
- tooltips are at most 360 px wide and wrap.

Call sites (`9f43bee`): the 236 px cap in `browser/annotate/send.rs` is gone and the agent's name is
truncated with a tooltip; the routes dialog, the GitHub account header, the placeholder error and
the canvas toast are clamped.

**Decision:** the tooltip is decided by measuring the label against the room the menu leaves. The
measure doesn't know the shortcut's real width, so a label can occasionally be cut without a
tooltip, or get one it doesn't need.

**Checked:** `crates/app/src/menu_tests.rs` (3 tests of truncation in the shared menu). Case A in
the real app was **not** checked: its menu needs a note, which needs a trusted, headed click.

### Cases B and C: the in-page note popover and agents' pills

**Built (`a5cc6af`):** item 16 removed the root cause. Then the overlay counter-scales by 1 / the
preview scale (a new `ui_scale` page op), so the popover, labels, markers and agents' pills look
the same at 49 % as at 100 %. The popover is `min(320, viewport − 16)` wide; segments use
`flex: 1 1 0; min-width: 0` with an ellipsis and turn 2×2 under 250 px; the footer wraps, buttons
never shrink, and the Esc/↩ hints drop under 270 px. Case C: the popover sits above agents, agents
take no pointer events, and a pill that would overlap the open popover moves to the other side, else
shrinks to a dot until the popover closes.

`f73d7d5` adds the same `uiScale` to mesurer's toolbar, ellipsizes long selectors in the element
label chip, and ellipsizes the agent pill's name.

**Checked:** a QuickJS unit test of the scale, width and overlap helpers; the headed e2e
`annotate-device: a click on {iPhone 15, iPad mini, Desktop} …` passes. Screenshots: before
`20261106-221526/annotate-shots/`, after `20261106-222907/annotate-shots/`.

**Not checked:** how mesurer's drag positions behave under CSS zoom in a running app.

### The audit (900 px window, long data)

| Surface | File | Result |
|---|---|---|
| All PopupMenu items, labels, links and submenus | vendor/gpui-component/src/menu/popup_menu.rs | fixed (shared) |
| Tooltips (shared) | vendor/gpui-component/src/tooltip.rs | fixed (shared) |
| Annotate "Send the notes to" / "Open with the notes" (case A) | browser/annotate/send.rs | fixed |
| send.rs button text at narrow widths | browser/annotate/send.rs | pass |
| Note popover in the page (case B) | core/browser/page/overlay/theme.js, popover.js, placement.js | fixed (a5cc6af) |
| Agent pill over the popover (case C) | overlay/agents.js, theme.js | fixed (a5cc6af); name ellipsis f73d7d5 |
| Overlay element label chip | overlay/theme.js, drawing.js | fixed |
| mesurer toolbar, panels and menus at preview scale | core/browser/page/mesurer/*, browser/mesurer/mod.rs | fixed |
| Canvas context, main and zoom menus, context bar, design panel dropdowns | board/menus/popup.rs, top_bar.rs, context_bar.rs, design_panel/* | pass (shared patch) |
| Canvas presence roster and edge chips | board/presence/roster.rs, edges.rs | pass |
| Canvas Actions palette, shortcuts sheet | board/menus/palette.rs, sheet.rs | pass |
| Canvas toast | board/toast.rs | fixed (line clamp) |
| Add Routes dialog | board/routes/dialog.rs | fixed |
| Canvas toolbar flyouts and template picker | board/toolbar/flyouts.rs, picker.rs | pass |
| Screens bar "Send to Agent…" | board/screens/bar.rs | pass |
| Canvas top bar icon buttons | board top bar | fixed: accessible names (290357c) |
| Workspace tab and group menus, "Move to Project" | workspace/tab_bar.rs | pass (shared patch) |
| Pages menu | workspace/pages_menu.rs | pass |
| Terminal tab and pane menus | terminal/panel/render.rs, terminal/tab/menu.rs | pass (snapped to the window) |
| Terminal "Copied" toast, find bar | terminal/view/render.rs, find/mod.rs | pass |
| Git branches menu | git/panel/branches.rs | pass (scrolls) |
| GitHub account menu | git/account/controls.rs, git/github/mod.rs | fixed (header) |
| Browser device menu, dev servers popover, presence chip, design colour menus | browser/device.rs, servers.rs, presence.rs, design/colors/mod.rs | pass |
| DB add, row and export menus; filter and selection Selects | db/panel/*, db/table/* | pass (tight, but no spill) |
| Catalog, new project, skills and package manager dropdowns; files, config and SSH menus | catalog/*, new_project_page, sawit_skills, package_managers.rs, files, shell_config, ssh | pass |
| MCP, skills and sign-in sheets; keymap form | mcp_page/*, skills_page/sheet.rs, keymap/editor/form | pass |
| Placeholder error detail | states.rs | fixed |
| Popovers (shared positioner) and context menus | vendor/gpui-base/src/positioner.rs, context_menu.rs | pass (flip and clamp) |
| Skills install sheet note | skills_page | fixed: read by screen readers (845a8de) |

**e2e:** `popups.test.js` (`f5ffef4`: menus with long data at a 900 px window, pictured to
`$SAWIT_E2E_SHOTS`) passes (fixui-e2e-2, after `290357c`). The two "covers" specs (menus over the
browser page and the canvas's live frame), which failed on `main` before the backlog, pass after
`1f658c6`.

**Not done:**
- The before/after screenshots of every fix are **not** on file: `sawit-wt/reports/popups/before`
  and `after` are empty.
- The audit's long data was checked at a 900 px window. Light and dark themes, iPad size,
  50–100 % panel zoom and a larger system font size were not each checked for every surface.
- A translated (longer) label was not tried.
