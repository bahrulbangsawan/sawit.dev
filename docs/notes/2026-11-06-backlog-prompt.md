# Sawit backlog: 30 changes (terminal, browser, canvas, keyboard, GitHub, tools, safety, polish, design handoff, live build cursors)

You are working on **Sawit**, a native macOS developer workbench written in Rust on GPUI
(`crates/app` is the UI, `crates/core` the logic, `crates/mcp` the MCP server, `crates/board`
the canvas engine, `vendor/gpui-base` the component library). Ship the items below one by one.
Each item says what is wrong or missing, where to start looking, what to build and how to know
it is done.

The file paths are starting points found by a quick survey. They are not a full map. Read the
code around them before you change anything, and keep to the conventions you find: module
layout, `tests/` folders beside the code, doc comments, the confirm-dialog and popover
components already in `vendor/gpui-base`, and how the MCP tools mirror app features.

---

## Ground rules for every item

1. **Work one item at a time, one commit per item** (Conventional Commits, as in `git log`:
   `feat(terminal): …`, `fix(browser): …`). Run `cargo fmt`, `cargo clippy --workspace` and
   the tests of the crates you touched before each commit. Do not push or release.
2. **Keep the app and the MCP server in step.** If a feature is something an agent would
   want (lock/pin a terminal, list ports, browser tabs, permissions…), add or extend the
   matching `sawit` MCP tool in `crates/mcp`, and mention it in the README's MCP table.
3. **Test in the real app.** Launch Sawit and use its own tools (`app_state`, `terminal_*`,
   `browser_*`, `canvas_*`, `browser_screenshot`) to check each change. Unit tests alone are
   not enough for UI work.
4. **Every destructive action asks first** (see item 13). This applies to everything you add.
5. **Popovers and menus must fit their content** (see item 15), and **only the user moves the
   keyboard** (see item 18). This applies to everything you add.
6. **Do not grow the binary or memory without a reason** (see item 7). Say what any new
   dependency costs in size.
7. **Ask before you guess.** If an item can be read two ways (item 9 for sure), stop and ask
   the user with a short, concrete question before you build.
8. When you finish, write a short report: what changed per item, how you checked it, what
   you could not do and why, and any follow-ups.

---

## A. Terminal

### 1. Copy error logs from the terminal and from instances

**Goal:** you can copy an error out of Sawit in one step and paste it into an issue or an agent.

- **Terminal:** add **Copy Error** / **Copy Last Output** to the terminal's context menu and tab
  menu (`crates/app/src/terminal/tab/menu.rs`, `terminal/view`). It copies the output of the
  last command (from its prompt to the next one, using shell integration or prompt marks if
  the terminal has them, else the visible screen). Lines are copied as plain text, with ANSI
  escape codes removed and wrapped lines joined back into one.
- **Instances** (local PostgreSQL and other managed services): add a **Copy Logs** button on the
  instance row or its log view. It copies the recent log (the last N lines, N about 200, set in
  one place). Add a **Copy Errors** variant that keeps only `ERROR`/`FATAL`/`PANIC` lines and
  their context.
- Show a short toast ("Copied 42 lines") when the copy succeeds.
- **Done when:** a failing command in a terminal and a crashed instance can each be copied in
  one click. The pasted text has no escape codes and keeps the original line order.

### 2. Lock, pin and close-all for terminals

- **Lock:** a locked terminal tab cannot be closed, by a click, by a shortcut or by an agent's
  `terminal_close`, until it is unlocked. Typing in it stays allowed. Show a lock icon on the
  tab.
- **Pin:** a pinned tab moves to the start of the tab bar, shows a compact (icon + short
  name) style, stays put when other tabs are reordered, and comes back pinned when the
  project is opened again (store it with the project's terminal state).
- **Close All Terminals** (and **Close Other Terminals**): in the terminal's right-click menus
  (see item 22, which lists the full menus) and the command palette. It skips locked tabs and says how many it skipped. It asks first if any tab has a
  running process (item 13).
- Add MCP support: `terminal_lock`/`terminal_pin` (or flags on an existing tool), and
  `terminal_close` must refuse a locked tab with a clear error.
- Files: `crates/app/src/terminal/tab/*`, `terminal/panel/*`, `workspace/tab_bar.rs`,
  `workspace/close.rs`, `crates/mcp/src`.
- **Done when:** lock, pin and close-all work from the mouse, the keyboard, the command palette
  and MCP. Locked tabs survive close-all. Pins survive a restart.

### 21. ⌘F search in files, Markdown, the browser, the canvas and the terminal

**Today:** ⌘F works on the Tools, MCP, Skills and New Project pages (`crates/app/src/catalog/mod.rs`)
and inside text inputs (`vendor/gpui-base/src/input/base/state.rs`: `cmd-f` → `Search`, with a
`search_session`). Whether the code editor already gets a find bar from that input has to be
checked. The terminal, the browser, the canvas and Markdown previews have no ⌘F.

**What to build:** one shared **find bar** component, used everywhere, that opens with ⌘F in the
view that has focus:
- A search field with the match count ("3 of 12"), **Previous** / **Next** (⇧↩ / ↩, ⇧⌘G / ⌘G), a
  **Match Case** toggle, a **Whole Word** toggle and a **Regex** toggle where the view supports it.
  Escape closes it. Closing it gives the keyboard back to the view (item 18).
- ⌘F with text selected starts the search with that text. ⌘F while the bar is open selects its text.
- It appears at the top right of the view it searches, never over the result it highlights, and fits
  the narrow panels (item 15).
- Every view highlights all matches, marks the current one more strongly, and scrolls it into view.

**Per view:**
- **Code and text files (editor):** use or finish the input's built-in search
  (`search_session`). Add **Replace** / **Replace All** (⌥⌘F) in editable files, not in read-only
  tabs (the "Make Tab Read-Only" state).
- **Markdown:** search the rendered preview when the preview shows, and the source when the source
  shows. If both show side by side, search the one that has focus.
- **Other file views** (PDF, images with text if any, `crates/app/src/viewers`): PDF search through
  its text layer. Views with no text say "Search isn't available here", rather than showing nothing.
- **Browser:** search the page with WebKit's own find (`WKWebView.find(_:configuration:completionHandler:)`
  on macOS 13+, or the private `_findString:` APIs as a fallback). Count and step through matches,
  and clear the highlight when the bar closes. ⌘F must reach Sawit's find bar even when the page
  has the keyboard (item 18). The page's own ⌘F handler (some apps have one) is not used.
- **Canvas:** search the text of layers on the current page (stickies, text, labels, layer names,
  and with a toggle all pages). Each match selects and zooms to the layer. Reuse the canvas's
  existing layer search if `crates/app/src/board/layers_panel` has one. Live web frames on the
  canvas are searched as in the browser only when they are entered.
- **Terminal:** search the scrollback as well as the screen (the terminal engine in
  `crates/core/src/terminal` / `crates/app/src/terminal/view`). Highlight matches in the grid, keep
  the highlights correct while new output arrives, and jump to a match in the scrollback. ⌘F
  must not be sent to the shell. Note: a terminal app running inside (vim, less, an agent CLI) may
  want its own keys. ⌘F belongs to Sawit, ⌃F stays with the program.
- Register `find`, `find_next`, `find_previous` and `replace` as keymap actions with these
  defaults, so users can rebind them and `keymap_list` shows them.
- **Done when:** ⌘F opens the same find bar in a code file, a Markdown preview, a PDF, the browser
  page, the canvas and the terminal. It finds, counts and steps through matches, and Escape gives
  the keyboard back to the same view. Tests cover the find bar's state, and terminal matching
  across wrapped lines and scrollback.

### 22. The terminal's right-click menus: as complete as the file tab menu

**Today there are two terminal menus, and both are thin:**
- **Right-click on a terminal tab** (`crates/app/src/terminal/panel/render.rs`, `.context_menu`,
  ~line 268): only **Rename Tab**, **Move to Project ▸**, **Remove from Group** and **Close**.
- **Right-click inside a terminal** (the pane menu, `crates/app/src/terminal/tab/menu.rs`,
  `pane_menu_items`): **Rename pane**, **Split right**, **Split down**, **Zoom**, **Send
  right-clicks to pane** and **Close pane**. The labels use sentence case, while the rest of the app
  uses Title Case.

The **file tab menu** (`crates/app/src/workspace/tab_bar.rs`, ~line 250–300) is the model to
follow. It has **Close ⌘W**, **Close Others ⌥⌘T**, **Close Left ⌘K E**, **Close Right ⌘K T**,
**Close Clean ⌘K U**, **Close All ⌘K W**, **Make Tab Read-Only** and **Pin Tab ⌘K ⇧↩**, each with
its shortcut shown on the right and grouped with separators. Reuse its `item(label, keys)`
helper and its `CloseOp`s. Don't write a second version.

**Terminal tab menu (right-click on the tab), grouped like this:**
1. **Rename Tab**, **Duplicate Tab** (new terminal in the same folder), **Move to Project ▸**,
   **Remove from Group**
2. **Close** ⌘W, **Close Others**
3. **Close Left**, **Close Right**
4. **Close Idle** (the terminal version of "Close Clean": tabs with no running process),
   **Close All**
5. **Lock Tab / Unlock Tab** (item 2: can't be closed while locked), **Pin Tab / Unpin Tab**
6. **Copy Last Output**, **Copy Error** (item 1), **Clear**, **Export Output…** (save the
   scrollback to a file)
7. **Full Screen** (item 17)

**Terminal pane menu (right-click inside the terminal):**
1. **Copy** ⌘C (off with no selection), **Paste** ⌘V, **Select All** ⌘A, **Find…** ⌘F (item 21)
2. **Copy Last Output**, **Copy Error** (item 1), **Open Link** / **Copy Link** / **Open File**
   when the click is on a URL or a path
3. **Clear** ⌘K, **Reset Terminal**
4. **Rename Pane**, **Split Right**, **Split Down**, **Zoom Pane / Unzoom Pane**,
   **Full Screen** (item 17)
5. **Send Right-Clicks to Pane** (checkbox)
6. **Close Pane**

**Rules:**
- Every item shows its shortcut, read from the keymap (`keymap_list`), as the file tab menu does.
  Items that have no binding yet show none. Add sensible default bindings where a clash-free one
  exists.
- Items that don't apply are disabled, not hidden, so the menu keeps its shape. For example,
  **Close Left** on the first tab, **Copy** with no selection, or **Unlock** on an unlocked tab.
- Destructive items follow item 13. **Close Others / Left / Right / All** skip locked tabs and ask
  first if a running process would be stopped, naming it ("Close 3 terminals and stop `bun dev`?").
- Use one label style: Title Case everywhere, the same verbs as the file tab menu.
- The menu must fit its content and follow item 15 (no spill-out, ellipsis for long project names
  in **Move to Project ▸**). It must hand the keyboard back to the terminal it was opened from
  when it closes (item 18).
- Add the same actions to the command palette and MCP (`terminal_close` with
  `others/left/right/idle/all`, `terminal_lock`, `terminal_pin`, `terminal_clear`).
- **Done when:** both menus show the items above with their shortcuts, each item works from the
  menu, its shortcut and the palette, the close items respect locks and ask before stopping
  processes, and a test covers which items are enabled for the first, last, locked and busy tab.

### 11. Fix keyboard focus when switching between the terminal, canvas and browser  *(bug, high priority)*

**Symptom:** after going back and forth between the terminal and the canvas or browser
(especially when both are open), keys stop working in the terminal. Tab, the tab-switching
shortcuts and sometimes every key do nothing until something else is clicked.

**Likely cause to check:** the canvas and browser are native `WKWebView`s placed over GPUI's
view (`crates/app/src/browser/engine/web_frame.rs`, `Host`). When one of them becomes first
responder, AppKit sends key events to the web view, not to GPUI. Going back to the terminal
moves GPUI's focus handle, but AppKit's first responder may never return to GPUI's view.
Look at `engine.focus()` / `focus_parent()` (`crates/app/src/browser/engine/mod.rs`),
`workspace/keyboard.rs`, `workspace/areas.rs`, `workspace/dock.rs` and the canvas's web views
in `crates/app/src/board/web`.

**What to do:**
1. Reproduce it on purpose, and write down the exact steps (e.g. click the browser page, click
   the terminal, press Tab).
2. Make one owner of "which panel area has focus". When a GPUI panel (terminal, editor,
   sidebar) gains focus, any web view that is first responder must give it back to GPUI's view
   (`window.makeFirstResponder(gpuiView)`). When a web view's area is clicked or activated,
   it takes first responder. Hidden or off-screen web views (tab not on show, settling off
   bounds) must never hold first responder.
3. App-wide shortcuts (switch tab, toggle panels, command palette) must work wherever focus is,
   including inside a web view. Route them through the app's keymap before the web view gets
   them, or have the web view hand them back.
4. Show which panel has focus (a subtle focus ring or a header tint), so it can be seen.
5. Add a regression test where it can be done (focus routing logic), and an `e2e/specs` flow
   if the e2e harness can drive it.
- **Done when:** after 20 rounds of switching between terminal, canvas and browser in any order,
  with mouse and with shortcuts, every key (Tab, Shift-Tab, arrows, Ctrl-C, Cmd-shortcuts)
  works in the panel that shows focus.

### 17. Full-screen hotkeys for the terminal, browser and canvas

- Add a hotkey that makes the focused panel (terminal, browser or canvas) fill the whole window,
  and the same hotkey (or Escape, when the panel doesn't need Escape itself) brings back the
  layout as it was. Suggested defaults, to be checked against the keymap for clashes:
  - **Toggle full screen for the focused panel:** `⌘⇧↩` (the way Terminal and iTerm zoom a pane).
  - **Full-screen terminal / browser / canvas directly:** `⌃⌘1` / `⌃⌘2` / `⌃⌘3` (or the
    `⌥`-number keys the canvas already uses for its panels, if those are free).
- Full screen means the panel takes the whole window area (sidebar, other docks, status bar
  hidden). It is not macOS's native full screen, which stays on `⌃⌘F`. Offer both if that fits
  better, and say why.
- Go through the existing code: `app_maximize_terminal` (`crates/mcp/src/app/window.rs:117`)
  already maximizes the terminal, so build on it and make it general. Look at
  `workspace/layout`, `workspace/dock.rs`, `workspace/areas.rs` and the keymap
  (`crates/app/src/keymap`, `crates/core/src/keymap.rs`).
- Register the actions in the keymap so `keymap_list` shows them and users can rebind them
  (`keymap_bind`). Show them in the command palette and in each panel's header menu, with
  the shortcut.
- The hotkeys must work when focus is inside a web view (browser page, canvas). Today, AppKit
  sends keys to the `WKWebView` there (see item 18), so make sure the app sees these keys first.
- While a panel is full screen, show a small hint ("⌘⇧↩ to exit") for a moment, and a button to
  exit.
- Add MCP: `app_maximize` with `panel: terminal | browser | canvas`, keeping
  `app_maximize_terminal` as an alias.
- **Done when:** each panel goes full screen and back with the hotkey, with the focus inside it
  (including inside a web page or a canvas text field). The previous layout comes back exactly,
  and the keys show in `keymap_list`.

### 18. Audit keyboard ownership: an agent's work must never take the user's keyboard  *(bug, high priority)*

**Symptom:** while an agent works (types into the browser page, drives the canvas, runs
commands), the user sometimes can't type in the terminal. Their keys land in the browser page
or the canvas instead. The same happens the other way round. This is related to item 11, but the
trigger here is the **agent's** activity, not the user's own clicks.

**What the code already tells you (starting points, check them):**
- `crates/app/src/board/web/live.rs`: when a live web frame becomes visible again (~line
  141–144), and in `web_enter` (~line 180), it calls `engine.focus()` **with no check that the
  user is in the canvas**. An agent's canvas op, or a re-render, can therefore make the web view
  AppKit's first responder while the user types in the terminal.
- `crates/app/src/browser/mod.rs`: `give_page_keyboard` (~line 390, called at ~259) gives AppKit's
  and GPUI's keyboard to the page "while the tab shows". Check whether agent actions
  (`browser_navigate`, `browser_click`, `browser_type`, `browser_press_key`) cause it to run.
- Other `engine.focus()` / `focus_parent()` callers: `browser/events.rs:82`, `browser/notes.rs:186`,
  `browser/viewport.rs:187`, `browser/panel.rs:261`.
- The comment at `browser/mod.rs:385–389` explains that GPUI sees ⌘ keys first, wherever AppKit's
  keyboard is. So a mismatch between AppKit's first responder and GPUI's focus sends keys to the
  wrong place.

**Rules to enforce:**
1. **Only the user moves the keyboard.** A click, a shortcut or a panel switch from the user can
   move focus. An agent's tool call, a page load, a re-render, a tab un-hiding, a canvas op or a
   terminal command must never move focus or first responder. Agent input goes to its target
   **programmatically**: JS events or WebKit's input APIs for `browser_type`/`browser_press_key`,
   and the PTY for `terminal_input`. It must never go by making that view first responder.
2. **One owner for focus.** A single "keyboard owner" (panel area plus GPUI focus handle plus
   AppKit first responder) that every focus change goes through, and that keeps AppKit and GPUI
   in step. If the owner is a GPUI panel, no web view is first responder. If it's a web view, GPUI's
   focus is on its host view.
3. **Hidden things never hold the keyboard.** An off-screen, settling, hidden or background web
   view (browser tab or canvas frame) gives first responder back as it hides.
4. **The terminal the user typed in last stays theirs.** Agent output in another terminal, or a
   new terminal opened by `terminal_run`, must not steal focus from the terminal the user is
   typing in. Decide whether an agent-opened terminal should be activated at all. If it is shown,
   it is shown without focus.

**What to do:**
1. Find every place that changes focus or first responder (`engine.focus`, `focus_parent`,
   `makeFirstResponder`, `window.focus`, `focus_handle.focus`, `.focus(window, cx)`, in
   `crates/app/src` and `vendor/gpui-base`). For each one, write down who triggers it (user or
   agent/system) and whether it is allowed under the rules above. Put the table in the report.
2. Reproduce on purpose. With the user typing a long line in a terminal, have an agent run in
   turn `browser_navigate`, `browser_type`, `browser_click`, `browser_press_key`, `canvas_add_elements`,
   `canvas_command`, a live web frame on the canvas, `terminal_run` and `app_open_page`. After each,
   check that the user's next key still goes to their terminal.
3. Fix the offenders, and route every focus change through the single owner.
4. Add tests: unit tests of the owner's rules, and an `e2e/specs` flow ("agent types in the browser
   while the user types in the terminal; the terminal gets every user key").
5. Show focus, as in item 11: the panel that has the keyboard is clearly marked, so the user can
   see where their keys will go.
- **Done when:** while an agent runs a scripted mix of browser, canvas and terminal tools in a
  loop for 2 minutes, a user typing in a terminal loses no keystrokes, and every keystroke lands
  there. The same holds with the user typing in the canvas or the browser page.

### 12. Closing a project closes its terminals

- When a project (window or project tab) is closed, every terminal that belongs to it closes
  too, and its child processes are stopped cleanly (SIGHUP, then SIGTERM, then SIGKILL after a
  timeout). Dev servers it started from the Run panel stop as well. No orphan processes or
  ports are left behind.
- If any of those terminals is running a process, or is locked (item 2), ask first: "Close
  *project* and stop 3 running processes?" and list them.
- Terminals that do not belong to a project (if there are any) are not touched.
- Files: `workspace/close.rs`, `workspace/project`, `terminal/panel/running.rs`, `crates/app/src/run`.
- **Done when:** after closing a project with a running `bun dev`, `list_ports` no longer shows
  its port and `ps` shows no leftover shell.

---

## B. Browser and canvas

### 6. List the active local dev ports in the browser

- In the browser's address bar (and in a small menu next to it), list the local servers that
  are listening right now, with port, process name and project (reuse
  `crates/core/src/ports` and `crates/app/src/ports`). Choosing one opens
  `http://localhost:<port>` in the current tab, or Cmd-click opens it in a new tab (item 10).
- Refresh the list when the menu opens, and keep scanning cheap (no polling while the menu is
  closed).
- **Done when:** with two dev servers running, both show in the list, and choosing one loads it.

### 10. Multiple tabs in the browser and the canvas

Today each project has one browser tab and one canvas. Make both multi-tab.

- **Browser:** a tab strip in the browser panel. New tab, close tab, reorder, Cmd-T / Cmd-W /
  Cmd-1…9 while the browser has focus. Each tab keeps its own URL, history, device mode and
  annotations. Only the tab on show keeps a live `WKWebView`. Background tabs are suspended or
  snapshot, so memory does not grow with the tab count (item 7).
- **Canvas:** several canvas documents per project (several `.sawit` files under `.nyawit/`,
  or several named canvases), each in its own tab, with rename and delete (delete asks first).
- **Agents and MCP:** the `browser_*` and `canvas_*` tools take an optional `tab` (id or name).
  Without it they use the active tab, as they do today. Add `browser_tabs` / `canvas_tabs`
  (list, open, close, activate). `app_state` lists the tabs.
- Restore tabs when the project opens again.
- Files: `crates/app/src/browser/*`, `crates/app/src/board/*`, `workspace/browser.rs`,
  `workspace/canvas.rs`, `workspace/pages.rs`, `crates/mcp/src/app/*`.
- **Done when:** three browser tabs and two canvases can be opened, switched, closed and
  restored, and an agent can act on a tab that is not on show.

### 16. Device preview inflates small text so lines overlap  *(bug, high priority)*

#### Symptom
With the browser tab set to a phone device (e.g. iPhone 15, 393×852), text smaller than
about 20 px renders bigger than the page asks for, but its line-height stays the same. Lines
in a paragraph touch or overlap. Large text (24 px and up) is fine. The same page renders
correctly in Safari and on a real iPhone.

#### Evidence (measured with `browser_evaluate` on a TanStack/Tailwind site)
- Preview state: viewport 393×852, device "iPhone 15", `scale: 0.4399237126509854`.
- `window.devicePixelRatio` in the page is `0.8798474073410034`. An iPhone 15 is 3, and
  anything below 1 is suspect.
- `getComputedStyle(document.documentElement).fontSize` is `20.458094px`, but no stylesheet
  rule sets a font size on `html` (every CSSOM rule was checked).
- 1,045 elements have a computed font size of exactly `20.458094px` and none is smaller, even
  though the CSS asks for 12/13/14/16 px.
- A `<p>` that the CSS sets to 14 px with a 20 px line-height computes to 20.458 px with a 20 px
  line-height, so the line-height is less than the font size.
- `-webkit-text-size-adjust: 100%` changes nothing, so text autosizing is not the cause.
- **9 / 0.4399237 = 20.4581.** The floor is WebKit's minimum font size (9 device px) divided
  by the preview scale.

#### Cause (already confirmed in the code; check it again before you fix it)
`crates/app/src/browser/viewport.rs` (`page_frame`, ~line 122–133) computes the fit scale and
calls `engine.set_zoom(scale)`. `crates/app/src/browser/engine/mod.rs:217` implements
`set_zoom` as wry's `webview.zoom(scale)`, which sets **`WKWebView.pageZoom`**. WebKit then
applies its minimum font size in device pixels, and at 0.44× that floor is 20.46 CSS px.

The canvas already solved this. `crates/app/src/browser/engine/web_frame.rs:194–218` has
`set_view_scale` (private layout mode `_WKLayoutModeDynamicSizeComputedFromViewScale` + `_setViewScale:`,
the way WebKit's own scaled previews work: the page lays out at the view's size ÷ scale and
text is not clamped). It also has `set_magnification` as a fallback. Its doc comment describes
this exact `pageZoom` clamp. The canvas uses them in `crates/app/src/board/web/live.rs` and
`board/screens/preview.rs`.

#### What to do
1. Confirm that the browser's device preview reaches `pageZoom` through `set_zoom`, and look
   for any other `pageZoom`, `magnification`, `minimumFontSize` or `devicePixelRatio` override.
2. Lay the page out at the device's true CSS size and scale only the visual output, in this
   order of preference:
   a. Reuse the canvas's approach: give the browser engine a `set_view_scale` path (the
      browser's engine and the canvas's `web_frame` may be separate wrappers, so share the code,
      don't copy it). Keep `pageZoom` at 1. The page must see `innerWidth` 393 and
      `innerHeight` 852.
   b. If `_setViewScale:` is missing, fall back to rendering the `WKWebView` at the device's real
      size and shrinking the view itself (`setMagnification` with a fixed layout size, or a scale
      transform on the view or its layer).
   c. Only if a zoom has to stay: set `WKPreferences.minimumFontSize` (and the internal minimum
      logical font size) to 0 while a device preview is on, and put the user's values back when
      it ends.
3. Report the emulated device's real `devicePixelRatio` (3 for iPhone 15, from the device
   profile), not a value derived from the fit scale, if the preview can control it (e.g. a
   private `_setOverrideDeviceScaleFactor:`). If it can't, say so in the report.
4. Clicks, hover, Annotate and Mesurer picks (`browser/annotate`, `browser/mesurer`,
   `browser/picking.rs`), element boxes, `browser_screenshot` (`browser/shots.rs`) and the
   caption's zoom percent must still map correctly. Their coordinate maths probably include the
   current scale, so check each one.
5. Add a test of `page_frame`/scale behaviour in `crates/app/src/browser/tests`.

#### Acceptance check (run in the preview at iPhone 15, at several panel sizes)
```js
const p = document.createElement('p');
p.style.cssText = 'font-size:12px;line-height:16px';
p.textContent = 'x';
document.body.append(p);
const c = getComputedStyle(p);
const r = { fs: c.fontSize, lh: c.lineHeight, dpr: devicePixelRatio,
            root: getComputedStyle(document.documentElement).fontSize,
            w: innerWidth, h: innerHeight };
p.remove();
r;
```
Expected at any panel size or scale: `fs: "12px"`, `lh: "16px"`, `root: "16px"`, `w: 393`,
`h: 852`, and `dpr` equal to the device profile's value. Today it returns
`fs: "20.458094px"` at scale 0.4399.

Also check that **Desktop** and **Responsive** modes, and the user's own browser font-size
settings, behave as they did before. Then click a link, annotate an element and take a
screenshot in device mode, and check that each lands on the right element.

---

## C. Tools, skills and integrations

### 3. MinIO and the MinIO client (`mc`) through Homebrew

- Add **MinIO** (`minio/stable/minio`) and **MinIO Client** (`minio/stable/mc`) to the Tools
  page catalog (`crates/core/src/dev_tools/catalog.rs`) with install, upgrade, uninstall and
  version detection (`detect.rs`, `versions.rs`), plus logos under `assets/tool_logos`.
  Watch out: Homebrew core's `mc` is Midnight Commander. Use the `minio/stable` tap and detect
  the right binary (`mc --version` says "minio").
- Ideal follow-up, if it fits the design: a MinIO **instance** like the PostgreSQL ones (data
  directory, port 9000, console port 9001, start, stop, logs, an `S3_*` / `AWS_*` block in
  `.env` via `write_env`). If you build it, give it the same lifecycle tools in MCP. If not, put
  it in the report as a proposal.
- **Done when:** both install from the Tools page and through `install_dev_tools`, and show
  their versions.

### 4. Add shadcn's skills to the Skills page

- `crates/core/src/skills/catalog.rs` already has one shadcn pack (`shadcn/improve`). Add
  shadcn's official UI skills (find the current repo and install command, e.g. `shadcn/ui`
  skills or the `npx shadcn` skill pack; check upstream, don't guess) as a catalog entry with
  author, home, repo, source and per-agent install. Update `crates/core/src/skills/tests/catalog.rs`.
- **Done when:** the pack appears on the Skills page and installs and removes for Claude Code
  and at least one other agent, also via `install_skill`.

### 23. Add Herdr to the Dev Tools install list

Add [Herdr](https://herdr.dev) to the Tools page catalog (`crates/core/src/dev_tools/catalog.rs`).
Herdr comes in two products, which install, update and live in different places. Add **both**
as separate entries:

| | **Herdr** (desktop app) | **Herdr TUI** (terminal app) |
|---|---|---|
| What it is | A native app for running coding agents. The agents run in `herdr-core`, a small server on the Mac, so they keep going when the app is closed. | The original Herdr: one binary with its own server. Its agents don't show in the desktop app. |
| Install (macOS) | `curl -fsSL https://releases.herdr.dev/app/install.sh \| sh` (or `Herdr.dmg` from `https://releases.herdr.dev/app/Herdr.dmg`) | `curl -fsSL https://herdr.dev/install.sh \| sh`. The docs also list `brew install herdr`, `mise use -g herdr` and Nix. |
| Platforms | macOS on Apple silicon only (also Windows and Linux) | macOS on Intel and Apple silicon, Linux, Windows |
| Binary / bundle | An app bundle (check whether it's `/Applications/Herdr.app`), plus the `herdr-core` server | `herdr` (the manual install puts it in `~/.local/bin/herdr`; check where the curl installer puts it) |
| Update | Check: the app's own updater, or running the install script again | `herdr update` for curl installs. Homebrew, mise or Nix installs update through their own manager. |
| Latest version | Check where releases are published (`releases.herdr.dev`) | GitHub releases of `herdrdev/herdr` (`Latest::GitHub("herdrdev/herdr")`) |
| Config | `~/.config/herdr-app/` (`settings.json`, `keys.json`, `layouts/`, `mods/`, `themes/`, `AGENTS.md`) | Check |
| Source / license | `github.com/herdrdev/herdr-app`, `herdrdev/herdr-core`, Apache 2.0 | `github.com/herdrdev/herdr`, Apache 2.0 |

**Check the unknowns, don't guess.** The website does not document these, so find them from the
install scripts (read them, don't just run them), the GitHub repos and a real install on this Mac:
- where each installer puts its files;
- the version command (`herdr --version`?) or the app bundle's `Info.plist` version, which the
  catalog's `version` field supports for apps;
- uninstall steps;
- whether `brew install herdr` is in Homebrew core or needs a tap.

Write what you found in the entry's comments.

**Catalog entries:**
- Use `Category::Agent` (Herdr runs coding agents), and a short description of each, in the style of
  the other entries. Add logos under `assets/tool_logos`.
- **Herdr TUI:** `bin: "herdr"`, the curl `install`, `upgrade: Some("herdr update")`,
  `latest: Latest::GitHub("herdrdev/herdr")`, and `owns` set to the curl installer's folder, so a
  copy installed by Homebrew, mise or Nix is detected but never removed by Sawit's uninstall.
- **Herdr app:** detected by its app bundle, versioned from `Info.plist`, installed with the app's
  install script. Gate it to Apple silicon: on an Intel Mac, show it as "Needs an Apple silicon
  Mac" instead of offering an install that fails.
- **Uninstall:** remove only what the installer put there, the binary or the app plus `herdr-core`.
  Stop `herdr-core` first, since it keeps running when the app is closed. **Never delete
  `~/.config/herdr-app/`** or the user's agents' work. Offer that as a separate, confirmed choice
  (item 13).
- Running agents: make clear in the uninstall confirmation that agents running in `herdr-core` will
  stop.
- Update `crates/core/src/dev_tools/tests` for both entries, and make sure `list_dev_tools`,
  `install_dev_tools`, `upgrade_dev_tools` and `uninstall_dev_tools` (MCP) see them.
- **Done when:** both show on the Tools page with their state and version. The TUI installs,
  updates and uninstalls from the page and through MCP. The app installs on Apple silicon and is
  marked unsupported on Intel. Uninstall leaves `~/.config/herdr-app/` in place.

### 24. Add HyperFrames to the Dev Tools install list (and its skills to the Skills page)

[HyperFrames](https://hyperframes.heygen.com) (by HeyGen, Apache-2.0,
`github.com/heygen-com/hyperframes`) makes videos with an AI agent: the agent writes the video as
code, and HyperFrames renders it in headless Chrome. It comes in three parts. Add each where it
belongs in Sawit:

| | **HyperFrames CLI** (Tools page) | **HyperFrames Studio** (Tools page) | **HyperFrames skills** (Skills page) |
|---|---|---|---|
| What | The `hyperframes` command: render, preview, `doctor`, skills, update | The desktop app; you sign in with a free HeyGen account | Agent skills that teach Claude Code, Codex, Cursor and others to make HyperFrames videos |
| Install | `npm install -g hyperframes` (also bun or pnpm global; Homebrew exists, since the docs say `brew upgrade hyperframes`. Check the formula or tap name) | Download from `https://hyperframes.studio/studio/download` (Apple silicon, Intel or Universal). Find the direct `.dmg`/`.zip` URL or a Homebrew cask | `npx skills add heygen-com/hyperframes` (choose **Core Skills**), or `npx hyperframes skills update` in the project folder |
| Update | `hyperframes update` (CLI and installed skills). npm, bun and pnpm installs also update themselves in the background. Homebrew: `brew upgrade hyperframes` | Check: the app's own updater or a new download | `npx hyperframes skills update` |
| Version | Check: the docs give `_meta.version` in `--json` output, but no `--version` flag | The app bundle's `Info.plist` | — |
| Needs | Node.js; **FFmpeg** (`ffmpeg`/`ffprobe`, for audio and media); **Chrome or Chromium** for headless rendering (`HYPERFRAMES_BROWSER_PATH` or `--browser-path` picks one) | A HeyGen account | An agent, plus the CLI (it runs through `npx`) |
| Health check | `hyperframes doctor` (and `media-use doctor` for FFmpeg) | — | — |

**Check the unknowns, don't guess.** Read the CLI docs (`hyperframes.heygen.com/packages/cli`,
quickstart, `studio-app.md`) and the repo, and install it on this Mac to confirm:
- the version command;
- the minimum Node version;
- the Homebrew formula or tap;
- the Studio app's bundle name and direct download URL;
- uninstall steps.

Write what you found in the entries' comments.

**Tools page entries** (`crates/core/src/dev_tools/catalog.rs`):
- **HyperFrames CLI:** `Category::Other` (or a new "Media" category if more tools like this are
  coming; say which in the report).
  - **Install method:** pick one, in this order: npm global if Node is installed, else the
    Homebrew formula if it exists.
  - **Dependencies:** `needs` lists Node and FFmpeg. Add **FFmpeg** (`brew install ffmpeg`) to the
    catalog if it isn't there. Install a missing dependency only after the user confirms.
  - **Updates:** `upgrade: Some("hyperframes update")`, and `latest` from npm or the GitHub
    releases.
  - **Ownership:** set `owns` so Sawit only uninstalls a copy it can remove (`npm uninstall -g
    hyperframes` for npm installs; a Homebrew copy through Homebrew).
  - **Browser:** if no Chrome or Chromium is found, say so and point to `HYPERFRAMES_BROWSER_PATH`.
    Don't install Chrome silently.
  - **After install:** offer **Run Doctor**, which runs `hyperframes doctor` in a Sawit terminal and
    shows its result.
- **HyperFrames Studio:** an app entry, detected by its app bundle and versioned from `Info.plist`.
  Install it from the download or the cask, then say that it needs a sign-in to HeyGen in the app.
  Sawit never handles that sign-in.
- **Uninstall:** remove only the CLI or the app. Never delete the user's video projects or renders.

**Skills page entry** (`crates/core/src/skills/catalog.rs`, as in item 4):
- Add a **HyperFrames** pack: author HeyGen, home `hyperframes.heygen.com`, repo
  `heygen-com/hyperframes`, source `heygen-com/hyperframes`. Install per agent with
  `npx skills add heygen-com/hyperframes` (Core Skills), and use the plugin route for Claude Code if
  the docs give one (the plugin's entry skill is `/hyperframes:hyperframes`; see
  `guides/with-claude-code.md`, `with-codex.md`, `with-cursor.md`, `plugins.md`).
- Updates run `npx hyperframes skills update`.
- Add tests in `crates/core/src/skills/tests/catalog.rs` and `crates/core/src/dev_tools/tests`.

**Done when:**
- The CLI installs, shows its version, updates and uninstalls from the Tools page and through MCP
  (`install_dev_tools`, `upgrade_dev_tools`, `uninstall_dev_tools`), and `hyperframes doctor` passes
  on this Mac.
- Missing FFmpeg or Chrome is reported, not hidden.
- Studio shows as installed or not, with its version.
- The skills pack installs and removes for Claude Code and at least one other agent, also through
  `install_skill`.

### 28. Add the "design-to-code" skill (Atharvsinh-codez/skills) to the Skills page

[`github.com/Atharvsinh-codez/skills`](https://github.com/Atharvsinh-codez/skills) (Atharv
Jadav, MIT) holds one skill, **design-to-code**. It turns a design image, a Figma frame or a live
page into responsive frontend code. Then it renders the result and compares it with the design,
and repeats until they match. Its `references/` folder holds playbooks the agent loads when needed:
reference-forensics, art-direction, interface-craft, typography-and-fonts, motion, verification and
evaluations.

- Add a pack to `crates/core/src/skills/catalog.rs`, the same shape as the shadcn entry:
  - name **Design to Code**, `by: "Atharv Jadav"` (or the GitHub handle), and `home` / `repo` set
    to `https://github.com/Atharvsinh-codez/skills`;
  - `source: "Atharvsinh-codez/skills"`, skill `design-to-code`, license MIT.
- **Install:** `npx skills add https://github.com/Atharvsinh-codez/skills --skill design-to-code`.
  The manual route copies the `design-to-code` folder into the agent's skills folder, at user or
  project level:
  - Claude Code: `~/.claude/skills/design-to-code/` or `<repo>/.claude/skills/`
  - Codex: `~/.codex/skills/`
  - Factory Droid: `~/.factory/skills/`
  
  Tell the user to restart the agent session after installing.
- Install only the skill folder. The repo also has six example Next.js sites in `websites/`, which
  must not be copied.
- It overlaps with the **build-design** skill Sawit's users may already have. Say so in its
  description, so people can choose.
- Add tests in `crates/core/src/skills/tests/catalog.rs`.
- **Done when:** the pack shows on the Skills page and installs, updates and removes for Claude
  Code, Codex and Factory Droid, also through `install_skill`/`uninstall_skill`, and only the
  skill folder lands on disk.

### 29. Add voxl (tembo/voxl) to the Dev Tools list

[`github.com/tembo/voxl`](https://github.com/tembo/voxl) is a Rust server that streams a desktop
(an X11 display, a Mac's screen or a Windows desktop) to a web browser. It sends H.264 video over a
WebSocket, and handles input, clipboard, audio and file transfer. It is made for headless VMs,
sandboxes and agents that need to see and control a desktop. `voxl` prints a sign-in link with a
token, on port 6901 by default. On a Mac it listens on `127.0.0.1` unless `--addr` says otherwise.

**How it installs (from the README; check it against the repo before you build):**
- There is no Homebrew formula, curl installer or npm package, and no prebuilt release yet.
- **Install route:** `brew install x264 opus pkgconf esbuild`, then `cargo install voxl-server`
  (the crate is `voxl-server`, the binary `voxl`). The README also lists libyuv as a build
  dependency, so check whether it is needed on macOS. Building from a clone
  (`cargo build --release`) also works.
- **Version:** `voxl --version`. **Settings:** command-line flags (`voxl --help`) and
  `VOXL_PASSWORD`. Update and uninstall aren't documented: use
  `cargo install voxl-server --force` and `cargo uninstall voxl-server`.
- **Permissions:** macOS asks voxl (or the terminal that starts it) for **Screen Recording** and
  **Accessibility**. An optional microphone needs a loopback device: `brew install blackhole-2ch`.
- **License:** the code is MIT, but a voxl binary links x264 and so falls under **GPL**.
  **Sawit must not bundle or ship a voxl binary.** It only installs it on the user's Mac, with the
  user's own build.

**Catalog entry** (`crates/core/src/dev_tools/catalog.rs`):
- `bin: "voxl"`, `Category::Other`, a description like "Streams this Mac's screen to a browser, for
  agents and remote control".
- **Install:** `brew install x264 opus pkgconf esbuild && cargo install voxl-server`. Set `needs` to
  Homebrew and Rust (rustup is already in the catalog). The build takes a while, so run it in a Sawit
  terminal (`interactive`) where the user can see progress.
- **Updates:** `upgrade: Some("cargo install voxl-server --force")`, and `latest` from crates.io (add
  a `Latest::Crate` variant if there isn't one) or from GitHub tags.
- **Uninstall:** `uninstall: Some("cargo uninstall voxl-server")`, with `owns` set to `~/.cargo/bin/voxl`.
  It leaves the Homebrew libraries alone, since other tools may use them.
- **Security, because this tool shares the screen and takes input:**
  - The tool's row says plainly what it does. After install, show the Screen Recording and
    Accessibility state with buttons to the right System Settings panes (shared with the
    permissions page, item 14).
  - If Sawit starts voxl (for example **Start** in the Run panel or a terminal), it binds to
    `127.0.0.1` only, with a strong random `VOXL_PASSWORD`. Never listen on `0.0.0.0` or a LAN
    address unless the user turns that on and confirms it (item 13), with a note that it needs a
    TLS proxy.
  - The sign-in link holds a token: never log it or send it to analytics.
- Add tests in `crates/core/src/dev_tools/tests`.
- **Done when:** on a Mac with Homebrew and Rust, voxl installs from the Tools page (and through
  `install_dev_tools`), shows its version, updates and uninstalls. It reports missing dependencies
  or permissions clearly. A test checks that Sawit starts it on `127.0.0.1` only.

### 5. Save prompts to GitHub

- The prompt library (`crates/app/src/prompts`, `crates/core/src/prompts.rs`) should sync to
  a GitHub repository. Reuse the existing GitHub sign-in and the private-repo backup code
  in `crates/core/src/github/backup.rs` (`own_repo`, `create_private_repo`, `commit_files`),
  which the shell config backup already uses.
- Settings: choose the repo (create a private `sawit-prompts` repo by default), **Back up now**,
  **Restore**, optional auto-backup on change. Store each prompt as a Markdown file with front
  matter (name, tags, variables), so the repo is readable on GitHub.
- When both sides changed, don't overwrite silently: show what differs and let the user choose.
- Add MCP: `backup_prompts` / `restore_prompts`, or a flag on the prompt tools.
- **Done when:** prompts round-trip (back up, delete locally, restore) with variables intact.

### 8. GitHub CLI (`gh`) installed by default, or one click away

- Add **GitHub CLI** (https://cli.github.com) to the Tools page catalog
  (`crates/core/src/dev_tools/catalog.rs`). It is **not** there today: `grep gh` in the catalog
  finds nothing. Fill in the entry like the others:
  - `id: "gh"`, name **GitHub CLI**, `bin: "gh"`, `Category::Other`, logo in `assets/tool_logos`;
  - install `brew install gh` (with Homebrew in `needs`), `upgrade: Some("brew upgrade gh")`,
    `uninstall: Some("brew uninstall gh")`;
  - `latest: Latest::GitHub("cli/cli")`, and the version from `gh --version` (first line,
    `gh version 2.x.y (date)`);
  - a copy installed another way (the `.pkg` from cli.github.com, MacPorts, conda) is detected
    but never uninstalled by Sawit, the same pattern as `owns` elsewhere.
- Both sign-ins live side by side: Sawit's own GitHub sign-in (item 19) and `gh auth`. Show
  whether `gh auth status` is signed in, and offer **Sign In with GitHub CLI** (runs
  `gh auth login` in a Sawit terminal). Don't copy Sawit's token into `gh` without the user's
  explicit choice.
- On first launch (or in the setup / welcome flow, `crates/app/src/welcome.rs`), if `gh` is
  missing, offer to install it, with a clear **Install GitHub CLI** button. Don't install
  software without asking. "By default" means it is preselected in setup, not installed
  silently.
- Wherever Sawit needs `gh` (git panel, PR actions, GitHub pages), show an inline **Install
  GitHub CLI** button when it is missing, instead of an error. Once installed, offer
  `gh auth login` in a Sawit terminal.
- **Done when:** on a Mac without `gh`, the user can install and sign in to it without leaving
  Sawit.

### 19. The GitHub sign-in is lost when the app is closed and opened again  *(bug, high priority)*

**Symptom:** the user signs in to GitHub in Sawit, quits the app and opens it again, and Sawit
shows them signed out. They expect to stay signed in until they press **Sign Out** or revoke
the app on GitHub.

**How it works today (check it):**
- After the device flow (`crates/core/src/github/sign_in.rs`), the token goes into the login
  Keychain: service `sawit`, account `github`, or `github:<SAWIT_HOME>` when `SAWIT_HOME` is set
  (`crates/core/src/github/token.rs`, `Keychain::user`, `user_account`). It is stored through
  `/usr/bin/security` (`crates/core/src/keychain.rs`).
- At launch, `GitHubAccount::load` (`crates/app/src/git/account/mod.rs:196`) reads it, then
  `confirm` asks GitHub whose it is. `confirmed` and `refused` **delete the Keychain item** when
  GitHub answers 401 (`github::is_revoked`, `http.rs:149`: every 401 becomes `REVOKED`).

**Suspects to check, in this order (prove which one it is before you fix it):**
1. **A Keychain error looks like "no sign-in".** `load` does
   `rx.await.ok().and_then(|r| r.ok()).flatten()`, so a Keychain *error* turns into `None` and
   then `Session::SignedOut`. Examples of such errors: a locked keychain, an access prompt that
   was denied or timed out, a `security` failure, or a legacy item that `get_legacy` can't read.
   The user then looks signed out though the item still exists. An error must show "Couldn't read
   your GitHub sign-in from the Keychain" with **Try Again**, and must never be treated as signed
   out.
2. **Any 401 deletes the token for good.** `refused` and `confirmed` call `store.delete()` on any
   401, so a 401 from an unrelated call can sign the user out and wipe the item. Examples: a
   request sent with a different or old token, a GitHub incident, or an endpoint that needs a
   scope the token lacks. Only delete when `GET /user` with *this* token returns 401. On other
   errors, keep the token and show the error.
3. **A different Keychain account per launch.** Check whether the user's normal launch sets
   `SAWIT_HOME` (dev runs, `cargo run`, e2e, the demo data, the CLI). Do the app and its helper
   processes read the same `service`/`account`? Is the item written with an ACL that a rebuilt or
   re-signed app can't read without a prompt (the `MARKER` / `security` comments in
   `keychain.rs`)?
4. **A sign-out on quit.** Check that no quit, window-close or project-close path calls
   `sign_out`, `refused` or `store.delete()`.
5. **The save itself fails quietly** after the device flow (the `save` result ignored), so only
   memory holds the token. Check that a failed save is shown to the user.

**Fix:**
- Read errors are not "signed out". Show a clear state with **Try Again**, and keep the item.
- Delete the stored token only on an explicit **Sign Out**, or when `GET /user` with that same
  token says 401.
- Stay signed in offline. With a saved token and no network, show the account as signed in but
  unconfirmed ("Can't reach GitHub"), as `Session::Unconfirmed` intends. Never show signed out.
- Log (without the token) why a session ended: user sign-out, 401 on `/user`, or a Keychain
  error. That way a report can show the cause.
- Add tests with the in-memory `TokenStore`: a load error keeps the token, a 401 from another
  endpoint keeps it, a 401 from `/user` removes it, and a restart with a saved token is signed in.
- **Done when:** sign in, quit (⌘Q), open again: still signed in. Repeat with the network off,
  after a rebuild of the app, and after 24 hours. App-side **Sign Out** is the only thing that
  signs the user out, apart from a real revocation on GitHub.

### 20. Shell files and GitHub: one clear button, and a two-way sync

**Today:** the shell config panel's footer (`crates/app/src/shell_config/github.rs`,
`render_footer`, ~line 110) shows **Save Shell Files to GitHub** (disabled behind
`account::gate` when signed out) **and** a **Sign in to GitHub** link
(`account::sign_in_link`, added `.when(blocked, …)`) at the same time. Two buttons for one
step confuse people.

**1. Show one button, chosen by the sign-in state:**
- **Signed out:** show only **Sign in to GitHub** (full width, the GitHub icon). Don't show
  **Save Shell Files to GitHub**, disabled or not.
- **Signing in / checking the account:** show the sign-in progress in the same spot.
- **Signed in, no backup repository yet:** show only **Save Shell Files to GitHub**. It creates the
  private `sawit-shell-config` repo (`BACKUP_REPO`, `crates/core/src/shell_config/backup.rs`).
- **Signed in, repository found:** show the sync controls below.
- **Keychain error / can't reach GitHub** (item 19): show the reason and **Try Again**, not the
  sign-in button.

**2. Sync when the repository is found:**
- When signed in, check whether `github.com/<login>/sawit-shell-config` exists
  (`github::own_repo`, as `plan_backup` does). Keep the existing check that refuses a public
  repo.
- Show the repo, the time of the last sync, and the state of each file compared with the repo:
  **Same**, **Changed here**, **Changed on GitHub**, **Changed on both**, **Only here**,
  **Only on GitHub**. Use content hashes against the last synced commit, so "changed on both"
  can be detected.
- Actions:
  - **Upload to GitHub** (local → repo): what `save_backup` does today. It commits the chosen
    files, keeps the existing secrets question (`secrets_question`, "Save lines that look like
    secrets?"), and shows a diff summary first.
  - **Download from GitHub** (repo → local): fetches the files from the repo and replaces the
    local ones. This is destructive, so it follows item 13: show a diff per file, let the user
    choose files, and confirm with "Replace 3 local files?". **Always back up** each local file
    first (e.g. `~/.zshrc.sawit-backup-<timestamp>`, or a backups folder Sawit manages), and offer
    **Undo** / restore from that backup.
  - **Changed on both:** never overwrite silently. Show both versions with a diff, and let the
    user keep local, keep GitHub's, or open both to merge by hand.
- After a download, tell the user which open terminals need `source ~/.zshrc` or a new shell,
  with a button to do it.
- Only files under the home folder that the backup knows (`backup_files`) are written. Never
  write outside them, and keep file permissions (e.g. `600`).
- Add the needed core functions next to `save_backup`: `fetch_backup` / `plan_sync` /
  `apply_download`, using the GitHub contents or git data API like `commit_files` in
  `crates/core/src/github/backup.rs`. Add tests with the files from fixtures.
- Add MCP: `shell_files_sync` with `status | upload | download`. `download` needs an explicit list
  of files and makes the backups first.
- **Done when:** signed out shows only **Sign in to GitHub**, and signed in shows only the right
  action. Edit `~/.zshrc` on another Mac (or on GitHub), then **Download** brings it in after a diff
  and a confirmation, with a backup beside it. Edit it locally, then **Upload** commits it. Editing
  both sides shows the conflict instead of overwriting.

### 9. Migrate "Claude pool" into Sawit  *(ask before building)*

This item is unclear. Before you write code, find out what "Claude pool" is on this machine,
for example the `~/.claude-pool/` folder (which `crates/core/src/agent_config` already treats
as a non-profile helper folder), a script, an app or a set of Claude profiles. Then ask the user
**what** should move into Sawit: the account/profile switching, the settings, the sessions, or
a whole tool. Ask too whether the old setup should be removed after. Write a short plan and get
a yes before you build. Never delete or move the user's config without a backup and a
confirmation.

---

## D. Safety, permissions and UI polish

### 13. Every destructive action asks for confirmation

- Make a list of every action that removes, closes, stops, overwrites or discards: remove an
  instance, connection, SSH host, prompt, skill, MCP server or tool; drop or truncate a database
  or table; close a terminal with a running process; close tabs or a project; delete a canvas or
  canvas page; discard git changes; stop processes; overwrite `.env`; clear the canvas, etc.
  Put the list in the report.
- Each one must open a confirmation dialog that names the thing ("Drop database **shop_dev**?
  This deletes all 14 tables.") and uses a red button with a specific verb ("Drop Database", not
  "OK"). Cancel is the default and Escape cancels. For the worst actions (drop a database, delete
  an instance with data), make the user type the name.
- Use one shared confirm component (extend the one in `vendor/gpui-base` or the app's existing
  dialogs; `grep -rl confirm crates/app/src` shows where they are used today). Don't write a
  new one each time.
- Closing a terminal tab with nothing running, or anything that can be undone, doesn't need a
  dialog. Use undo or a toast with **Undo** where that is better than a dialog.
- MCP tools that destroy things keep working for agents (the agent's own permission layer is
  in charge there), but must stay clearly named and documented as destructive.
- **Done when:** every item on the list asks first, and a test covers the shared component.

### 14. A permissions management page

- A new page (next to the MCP, Skills and Tools pages, under `workspace/pages.rs`) where the
  user sees and edits what agents are allowed to do:
  - each agent's permission settings on this Mac (Claude Code's `settings.json`
    `permissions.allow`/`deny`/`ask` at user, project and local levels, and the same for other
    agents that `crates/core/src/agent_config` / `mcp_clients` already detect);
  - which `sawit` MCP tools need confirmation and which are allowed, especially the destructive
    ones from item 13;
  - macOS permissions Sawit itself needs (Accessibility, Automation, Files & Folders, Screen
    Recording, if any), with the current state and a button that opens the right System
    Settings pane.
- Edits are safe: back up a file before changing it, keep its formatting and unknown keys, show
  a diff before saving, and never write secrets into it.
- **Done when:** the user can add, remove and move a rule between allow/ask/deny for Claude
  Code at user and project levels from the page, and the file on disk changes as shown.

### 15. Fix every popover, dropdown and menu whose buttons and text spill out  *(high priority)*

**Goal:** no text or button in any popup is cut off, runs past its edge, overlaps another
control or is covered by something else, in any theme, panel size or device mode. This item is
not done until every popup in the app has been checked, not only the three cases below.

#### Cases the user reported (screenshots from 2026-11-03)

**Case A: the annotate panel's "Send the notes to" agent menu** (GPUI, in the browser's notes panel)
- The menu lists running agents as `Claude Code · ◑ FormValiditySummary card desi…`. The label
  runs straight into the menu's right edge and is cut mid-letter, with no ellipsis and no right
  padding. The full name can't be read anywhere.
- The menu covers the row under it ("Clear all"), and the split button's chevron sits close to
  the **Copy** button.
- **Code:** `crates/app/src/browser/annotate/send.rs`. `MENU_WIDTH = 236.` (line ~24) caps the
  menu with `menu.max_w(px(MENU_WIDTH))` (lines ~218 and ~268). The comment says this is because
  "the page's webview draws above it". Commit `836be2b` made menus draw above the web views, so
  the cap's reason is gone. The items are `gpui_kit::component::menu::PopupMenuItem`, which
  don't truncate long labels.
- **Fix:** truncate the item label with an ellipsis (a single line, the session name shortened
  and the agent name kept), keep right padding, show the full name in a tooltip. Let the menu
  grow to fit the content, up to the space in the window (`min(content, window − margins)`), not a
  fixed 236 px. Do the truncation in `PopupMenuItem` / the menu component in `vendor/gpui-base`,
  so every menu in the app gets it. Also check `send.rs:329` ("a button's label beside its logo,
  cut short when the panel is narrow") at narrow panel widths.

**Case B: the annotate note popover in the page** (HTML/CSS overlay injected into the page)
- At a phone device size, the popover's text is too large for its box. The segmented control
  `Fix | Change | Question | Copy` touches the right edge. In the footer, the `Esc` hint runs into
  the **Cancel** button, and **Save ↩** is pushed against the edge or cut ("Cancel E…").
- **Code:** `crates/core/src/browser/page/overlay/theme.js` (`.pop` has `width: 320px`, text
  11–12 px), `popover.js`, `placement.js` (clamps the popover inside the viewport), and
  `crates/app/src/browser/annotate/card.rs`.
- **Likely cause: item 16.** The overlay lives inside the page, so the device preview's
  `pageZoom` applies to it. The box width shrinks with the zoom, but WebKit's minimum font size
  pushes its 11–12 px text up to ~20 px. So the text no longer fits a box laid out for 12 px. Fix
  item 16 first, then check this popover again.
- **Then make the overlay solid on its own:**
  - Draw Sawit's overlay at a constant on-screen size, whatever the device zoom. For example,
    counter-scale the overlay root by `1 / scale`, or keep it in its own layer outside the zoomed
    page. A note popover should look the same at 100% and at 44%.
  - Make the layout robust: the segmented control's segments use `flex: 1 1 0; min-width: 0`
    with an ellipsis. The footer wraps or drops the `Esc`/`↩` hints when space is short, instead
    of letting them collide. Buttons never shrink below their label. The popover's width is
    `min(320px, viewport − 16px)`, and its contents fit that width.
  - Protect the overlay from the page's CSS: Shadow DOM or a full reset, a fixed `font-size` on
    the overlay root, `-webkit-text-size-adjust: none`, `box-sizing: border-box`.

**Case C: an agent's presence label covers the user's popover**
- While an agent works on the page, its cursor and pill (`Agent ▌ typing in Email`) draw on top
  of the open note popover, over the **Save** button and the footer.
- **Code:** `crates/core/src/browser/page/overlay/agents.js` and the `.agent`, `.pill` and `.caret`
  rules in `theme.js`.
- **Fix:** the user's open popover (and anything the user is typing in) must always sit above
  agent presence. Agent cursors and pills never take clicks (`pointer-events: none`). When an
  agent's pill would overlap the open popover, move it to the other side or fade it to a small
  dot until the popover closes. Never cover a button the user can press.

#### Audit everything else
- Make a list of every popup surface: GPUI menus, context menus, dropdowns, selects, comboboxes,
  popovers, tooltips, sheets, dialogs, toasts and command palettes in `crates/app/src` and
  `vendor/gpui-base` (`menu`, `popover`, `select`, `dropdown`, `tooltip`, `dialog`, `sheet`…),
  plus every HTML overlay injected into pages (`crates/core/src/browser/page/**`: overlay,
  design, mesurer) and the canvas's own menus (`crates/app/src/board/menus`). Put the list in the
  report with a pass/fix result for each.
- For each one, check for:
  - text that runs past its container or is cut without an ellipsis;
  - buttons, hints (`Esc`, `↩`, shortcuts) or icons that overlap or touch an edge;
  - popups that go off the window or panel edge (they must flip or shift, as `placement.js`
    tries to);
  - popups that cover the control that opened them, or are covered by a web view, an agent
    overlay or another panel (z-order);
  - layouts that break in light and dark themes, at narrow panel widths, at device-preview
    zoom levels, and with the system font size made larger.
- Fix it in shared components first: content-fit width with min/max, single-line ellipsis plus a
  tooltip, consistent padding and row height, room for a trailing shortcut hint, flip/shift
  placement. Then fix the call sites that still break.
- Use long data on purpose: a long project name, branch name, file path, agent session title
  (60+ characters), a selector like `div.mt-6.rounded-cards.some-long-class`, and a translated
  (longer) label.
- Take a before/after `browser_screenshot` or window screenshot of every fix, including the
  three cases above at iPhone 15 and Desktop sizes.
- Don't break commit `836be2b` (menus show above the browser and canvas web views).
- **Done when:** cases A, B and C are fixed and look right at iPhone 15, iPad and Desktop sizes
  and at 50%–100% panel zoom. Every popup on the audit list passes at a 900 px wide window with
  long names. A test covers truncation in the shared menu component.

---

## E. Size and memory

### 7. Measure size and memory, then make Sawit as small as possible without losing features

1. **Measure first** and record a baseline in the report:
   - release binary size (`cargo build --release`), the `.app` bundle size and what is in it
     (assets, fonts, icons, embedded JS);
   - `cargo bloat --release --crates` and `--filter` the top crates; `cargo tree -d` for duplicate
     dependency versions;
   - memory at idle, with one project, with a terminal + browser + canvas open, and after 30
     minutes of use (RSS of the app and of each WebKit content process).
2. **Then shrink**, cheapest and safest first:
   - release profile: `lto = "fat"`, `codegen-units = 1`, `opt-level` compared ("3" against "s"
     against "z", measured, not guessed), `strip = true`, `panic` strategy as it stands unless
     measured;
   - remove duplicate crate versions, turn off unused default features, swap heavy crates for
     lighter ones where the feature stays the same;
   - compress or load assets lazily (`assets/*` logos and icons: SVG through an optimiser, raster
     to the smallest format that keeps quality), and drop unused ones;
   - memory: release background web views (item 10), cap terminal scrollback sensibly, drop
     caches that are not used, look for leaks in long sessions.
3. **No feature may be lost.** All tests must pass, and the e2e specs must still pass.
4. Report a before/after table (binary, bundle, idle RSS, busy RSS) with every change that
   helped, and leave the ones that didn't help out.

---

## F. Canvas to code: export, handoff and the design standard

Background: v0.0.9 (`cc67f33`) added **screens**. A screen is a UI built as live HTML in a
frame (`canvas_begin {kind: "screen"}` + `canvas_screen_write`, with its files in
`.nyawit/screens/<screen>/`). When one is selected, a bar shows Preview, Download HTML,
Download React and Send to Agent… (`crates/app/src/board/screens/bar.rs`, `handoff.rs`,
`crates/board/src/screens/export`). An **artboard** is a frame built from vector layers
(`canvas_add_elements`, `canvas_command`), and it gets none of that: it exports only as
SVG/PNG/PDF and Copy as text/CSS/SVG/PNG (`crates/app/src/board/commands/mod.rs`,
`crates/board/src/ops/copy_as.rs`). Users who ask for a UI often end up with an artboard,
because agents draw shapes, and then they can't export or hand off the design.

### 25. Export buttons for HTML and React on screens and artboards

1. **Make the buttons easy to find and the same everywhere.** Each of these places should
   offer **Export HTML** and **Export React**:
   - the bar over a selected screen (it already has Download HTML / Download React; keep the
     labels the same across all three places);
   - a new bar, in the same place and style, over a selected top-level artboard (a frame
     on the page whose children are layers);
   - the frame's right-click menu and File ▸ Export (with the selection's name), next to the
     SVG/PDF entries;
   - the design panel's Export section, as two more formats beside PNG/SVG/PDF.
2. **Artboard → code.** Add a converter in `crates/board` (next to `screens/export` and
   `ops/copy_as.rs`) that turns a frame's layer tree into:
   - **HTML**: one self-contained file. Auto-layout frames become flexbox (direction, gap,
     padding, alignment, hug/fill/fixed sizing), the rest are absolutely positioned. Text
     keeps its font, size, weight, line height, letter spacing and color. Fills, strokes,
     radii, shadows and opacity become CSS. Images and vector icons are embedded (data URI or
     inline SVG). Color and type tokens, where the doc has them, become CSS custom properties.
     Layer names become class names and `data-name`.
   - **React**: one `.tsx` component (named the way `screens::component_name` names them) and
     its CSS module, the same output as the HTML. Components and instances on the canvas
     become React components with props for their variant properties, where that is
     practical.
   - Reuse the CSS that Copy as CSS already produces per layer. Don't write a second
     style mapper.
   - Use the same default paths as screens (`.nyawit/exports/<name>.html`, `<Name>.tsx`), and
     ask where to save, starting in the project folder, as the screen downloads do.
   - The output is a **mechanical reference**, not production code; say so in a comment at
     the top of each file, as the screen React export does.
3. **The skill cites a command that doesn't exist.** `skills/sawit-canvas-design/SKILL.md`
   (rule **cnv.3**) tells agents to use `inspect.code {id, language: "react" | "html" |
   "tailwind"}`, but no such command is in `crates/board` (check `canvas_commands`). Implement
   it on top of the converter from step 2 (with `tailwind` as a third output), or fix the
   rule. Don't leave agents calling a command that isn't there.
4. **MCP.** Make `canvas_screen_export {format: "html" | "react"}` also accept an artboard
   (by `id` or name), or add `canvas_frame_export`. Either way an agent can do from MCP
   anything the buttons do. Update the README's MCP table.
5. **Done when:** a vector artboard and a screen both export to HTML and React from the bar,
   the right-click menu, File ▸ Export and the design panel; the HTML opened in Sawit's browser
   looks like the artboard at 1:1 (compare `browser_screenshot` with `canvas_export` png, which
   should differ only by anti-aliasing for auto-layout frames); the `.tsx` type-checks in a
   fresh Vite + React + TS app; the tests cover auto layout, text styles, images, an instance
   with variants, and a frame with absolute children.

### 26. A handoff button for screens and artboards

1. **Send to Agent… works on artboards too.** Today only the screen bar has it
   (`screens/handoff.rs`). Add it to the artboard bar from item 25, to the frame's
   right-click menu, and to a multi-selection (hand off several screens/artboards as one
   flow, in canvas order, with the prototype links between them if there are any).
2. **What the handoff writes**, in `.nyawit/handoff/<name>/` as for screens:
   - `design.png` (at 2x) and, for artboards, the HTML and React from item 25;
   - `spec.md`: frame size and breakpoints, the layer tree with names, the tokens used
     (colors, type, spacing, radii) with values, the components and variants, the copy, and
     the states that are drawn (empty, loading, error, hover…);
   - `prompt.md`: the prompt `handoff::prompt` builds today, which names the files above and
     asks the agent to build the screen **in the project's stack and components** (not by
     pasting the export), following the design standard (item 27), and to check the result
     against `design.png` in Sawit's browser.
3. **The menu.** Keep the screen bar's menu: the coding agents found on this Mac (Claude Code,
   Codex, …) open in a new terminal tab in the project with the prompt; **Copy Prompt** puts it
   on the clipboard; add **Reveal Handoff Folder**. Show a toast with the folder when it's
   done, as exports do.
4. **MCP.** Add `canvas_handoff {id | work | screen, agent?}`, which writes the same folder and
   returns the paths and the prompt, so an agent can hand a design to another agent (or to
   itself in a new session) without the UI. List it in the README's MCP table.
5. **Done when:** handing off a vector artboard and a screen to Claude Code each open a
   terminal tab whose first message names the folder, and the agent builds a page that matches
   `design.png` in Sawit's browser; Copy Prompt and Reveal work; tests cover the folder's
   contents and the prompt for one artboard, one screen and a two-frame flow.

### 27. The design standard is on by default: no install, and applied to every UI an agent builds

The standard is already **bundled in the binary and served over MCP**:
`canvas_design_guide` returns `sawit-canvas-design/SKILL.md` from
`sawit_core::skills::bundled` (`crates/mcp/src/app/canvas/guide.rs`), and the server's
`INSTRUCTIONS` (`crates/mcp/src/lib.rs:105`) tell agents to read it first. So the user
doesn't have to install the skill. But it is **only advice**: an agent that skips
`canvas_design_guide` builds UI without the rules, and nothing checks what it built.
Make the standard apply without the agent having to cooperate:

1. **Send the rules with the work.** The first `canvas_begin` of a UI (`kind: "screen"`, or a
   frame named or sized like a screen) in a session answers with a short digest of the rules
   (the must-follow rules and the checklist, ~1–2k tokens, generated from SKILL.md at build
   time so it can't go stale) and a pointer to the full guide by section. Later calls answer
   with the section the work needs (forms, tables, navigation…) when the agent names one in
   `steps`. Never send the whole guide on every call.
2. **Steer UI to screens.** UI built as vector shapes can't be previewed live, exported or
   handed off as well as a screen. Make `kind: "screen"` the documented default for UI in
   `INSTRUCTIONS`, the `canvas_begin` description, rule **cnv.3** and `skills/sawit-canvas/
   SKILL.md` (which doesn't mention screens today). When an agent builds a screen-sized frame
   from shapes, say in the reply that it should use a screen instead. Keep vector for diagrams,
   wireframes the user asked to be sketchy, and illustrations.
3. **Check what was built.** After each `canvas_screen_write` (and on `canvas_progress
   {status: "done"}` for artboards), run the rules that can be checked by a machine: text
   contrast (reuse `skills/sawit-canvas-design/scripts/contrast.py`'s formula in Rust), touch
   target size, minimum text size, spacing on the token scale, images without alt text, inputs
   without labels, missing focus styles, text that overflows its box. Return the failures in
   the reply as `{rule, layer/selector, fix}` with the rule ids from SKILL.md, so the agent
   fixes them before it says it's done. Show the same list on the canvas as a badge on the
   frame, which the user can open.
4. **Expose it as MCP prompts and resources too.** Clients that support them get the guide as
   an MCP resource (`sawit://design-guide`, and a resource per section) and a `design-ui` MCP
   prompt, so it can be loaded without a tool call. Keep `install_sawit_skills` for agents that
   want it as a local skill, but nothing should depend on it.
5. **Keep `INSTRUCTIONS` under 4,000 characters** (see the comment above it): add a line, don't
   grow it by paragraphs.
6. **Done when:** a fresh agent session with no Sawit skills installed that never calls
   `canvas_design_guide` and is asked to "design a settings page on the canvas" builds it as a
   screen, gets the digest on `canvas_begin`, gets rule failures back on its writes, and ends
   with none left; a test checks that the digest matches SKILL.md; a test seeds a
   low-contrast, 10 px text, 30 px button screen and expects those three failures.

### 30. Live build cursors: every agent's cursor, and every element animated as it is built

The goal: when agents work on the canvas, it should look like a multiplayer design file with
real people in it. Each agent (and each subagent) has its own cursor, and each one visibly
drags out boxes, types text, draws lines, duplicates repeated items and turns frames into
screens. The motion should come from the result being drawn, never from the result waiting
on the motion.

**Prior art and licence.** The pattern is inspired by
[swarm-cursors](https://github.com/DhvanilPatel/swarm-cursors), itself a tribute to Joel
Califa's [Swarm](https://califa.github.io/swarm-site/). **Neither repo has a licence** (the
GitHub licence API returns 404 for both), so their code, cursor art and sounds are all
rights reserved. **Do not copy or port `swarm.js`, its cursor set or its timings.** Take
the general ideas only (blueprint → scripted tasks per element → dependency order → fade to the
live result) and write Sawit's own version in Rust/GPUI, with Sawit's own cursor art and
timings. Credit the inspiration in `CREDITS.md`. If we ever want their exact look, ask the
authors first.

**What exists today** (build on it, don't replace it): `crates/app/src/board/presence/` has
one pointer per agent (10 × 14 px, colour + white edge, glides for `GLIDE` to the corner of the
work), name pills, a skeleton and a row per work, ghost placeholders for slow ops, a roster,
off-screen chips, polite announcements and `reveal.rs` (text revealed word by word from a
draw-only copy of the document). The agent identity is in `sawit_core::agents`
(`AgentIdentity`, `Roster`) and `crates/mcp/src/agent.rs`.

#### 30a. Many agents, many cursors (the bug: only one cursor shows)

**Why:** an identity is **one `sawit-mcp` process** (`crates/mcp/src/agent.rs`: "a session's
subagents share it"). Claude Code's subagents, workflows with many agents, and parallel tool
calls from one session all go through the parent's process, so they all share **one** id,
**one** colour and **one** pointer, which jumps between their works. Several real agents look
like one.

1. **Split the identity into session + lane.** Keep `AgentIdentity.id` as the session, and
   add an optional `lane` (sub-agent id), `lane_name` and `parent`. The roster keys cursors by
   `(id, lane)`. A lane gets its own colour from the same palette (keep enough contrast between
   neighbours, and don't reuse a colour while it's on screen) and its own pointer, pill and
   row. The roster and the off-screen chips group lanes under their session
   ("Claude Code · 4 agents").
2. **Where a lane comes from**, in this order:
   - an explicit `agent` argument on every canvas tool (`{id, name?}`, e.g.
     `{id: "header", name: "Header agent"}`), documented in the tool descriptions and
     `INSTRUCTIONS` as "pass a stable `agent` id when you are one of several agents"; add a
     line to the `sawit-canvas` skill telling orchestrators to give each subagent its own id;
   - else the `work` from `canvas_begin`: each open work of a session is its own lane, named
     after the work's author if given, otherwise "<client> 2", "<client> 3";
   - else whatever MCP request metadata the client sends that tells subagents apart (look
     at what Claude Code, Codex and Cursor put in `_meta` / progress tokens today, and use it
     if it's stable);
   - else the session itself, as today.
3. **Concurrency.** Ops from different lanes can arrive in the same frame. Each lane has its
   own animation queue (30c), so they all play at once, never one after another. Two
   lanes on the same frame must not draw on top of each other: offset their pointers, and
   stack their rows.
4. **Lifecycle.** A lane appears on its first op, glides in from the edge nearest its target
   (or fades in under reduced motion), goes idle with its work, and leaves after `GONE_AFTER`
   like an agent does. When a session ends, all of its lanes go.
5. **Done when:** a Claude Code session that starts 4 subagents, each building a different
   component with its own `agent` id, shows 4 cursors in 4 colours at once, each at its own
   component, each with its own row and name; the roster says 4; a test feeds interleaved ops
   from 4 lanes of one session and expects 4 cursors and 4 queues; a session without `agent`
   but with 3 open works shows 3 cursors.

#### 30b. The cursor itself

1. **Art.** Draw Sawit's own pointer set as vectors (crisp at every zoom and on 1x/2x screens),
   always shown at the same screen size: arrow (moving / selecting), crosshair (drawing a
   box), I-beam (typing), pen (lines and vectors), move/⌥-duplicate (arrow with a +), hand
   (resizing handles optional). Fill in the agent's colour with a 1.5 px white edge and a soft
   shadow, as today. The name pill sits at the tip, 11 px semibold, in the agent's colour.
   While the agent works, the pill can also show what it's doing ("Header agent · typing").
2. **Motion.** The pointer travels between targets on a slightly curved path (a quadratic
   Bézier with a small sideways offset, eased in and out, duration scaled to the distance and
   capped, e.g. 120–450 ms). It never teleports, except under reduced motion, where it jumps.
   It shows a small press/release dip on click. Idle pointers drift a few px and rest; never
   loop forever when nothing happens (save CPU, see 30f).
3. **Never the user's cursor.** These are painted marks only: they never move the system
   cursor, take focus, change the selection, view, page or keyboard (items 18 and the
   presence rules), and they are invisible to hit testing. The user can work underneath.

#### 30c. The replay engine (optimistic, never blocking)

An MCP call arrives whole, so the result is known before any motion starts. Therefore:

1. **Commit first.** Every op is applied to the document at once, as today: saved, undoable,
   visible to other agents and to `canvas_get`. The animation is a **presentation layer**
   over it and never changes the document.
2. **Draw a hidden copy.** Generalise `reveal.rs`: each lane holds a *replay* of the layers its
   last ops added or changed. The canvas draws the document with those layers masked to their
   current animation state (hidden, placeholder, growing box, partially typed text…). Draw
   cheap moving parts (growing rects, badges, guides, pen rubber bands, caret) as **overlay
   marks** in `presence/paint.rs`, and rebuild the masked document copy only when an
   element reaches a milestone (box placed, fill landed, word typed). Never rebuild it every
   frame.
3. **Script per op.** Turn each op's diff (`presence/target.rs` already computes what was
   added, changed or removed) into a list of tasks, ordered so that a parent frame comes before
   its children, a master before its duplicates, and top-left before bottom-right in reading
   order. Each task is a small state machine with a duration (see 30d).
4. **Catch up, never lag.** Each element has a time budget, each op has a total budget
   (e.g. 2.5 s at 1×), and each lane has a queue limit. When a new op arrives for a lane
   that is still playing, speed up what's left (2× → 4× → snap). If the user scrolls an
   element into view, selects it, edits it or exports it, it snaps to the final state at once.
   Screenshots, `canvas_export`, `canvas_screen_get` and saving always see the real document,
   never a half-played frame.
5. **Undo/redo and failures.** Undo while a replay is playing cancels that replay. A failed
   op plays nothing and shows the failed state that exists today.

#### 30d. Element animations

Make each of these look like a skilled designer doing it, in the agent's colour:

| Element / op | Animation |
|---|---|
| Frame / artboard / screen frame | Crosshair drag from the top-left to the bottom-right corner, a live `W × H` badge, a light outline, then the frame's fill and name fade in. |
| Rect, ellipse, polygon | Crosshair drag with the `W × H` badge, grey placeholder, then fill, stroke, radius and shadow fade in (≤ 150 ms). |
| Auto-layout children | Each child is dragged into place in order; the parent's padding and gap show briefly as pink spacing marks. |
| Text | Drag a text box (or click for auto-width), I-beam, type with a blinking caret at a speed that scales with length (cap at the text budget; long text types its first line and fills in the rest). Headings can start small and step up to their size. Replaces `reveal.rs`'s word reveal. |
| Text edit | Select the old text (highlight), then type the new text. |
| Image / media | Placeholder box, then the image wipes in from top to bottom. |
| Line, arrow, connector, vector | Pen cursor: anchor points click in, the rubber band follows the cursor, and the curve handles show briefly. |
| Duplicates (repeated cards, list rows, nav items) | Select the master, ⌥-drag a copy with red distance marks, drop, then retype its text / swap its image. Detect repeats the same way for artboards (same component or same structure) and screens (30e). |
| Component / instance | A purple component outline; instances get the ◇ badge as they land. |
| Move / resize / restyle (`canvas_update`) | The pointer grabs the layer and drags or resizes it from its old state to its new one; colours cross-fade. |
| Delete | The pointer selects it, and it fades out with a short shrink. |
| Guides and alignment | When children share left edges or baselines, a guide slides out from the ruler and the text snaps onto it. |
| Mermaid / diagram | Nodes are drawn as boxes in the order of the graph, edges with the pen. |
| Sticky / whiteboard | A sticky is placed with a small bounce, and its text is typed. |

Respect the theme (light/dark canvas), the zoom (badges and marks keep their screen size) and
clipping (nothing draws outside a clipped frame).

#### 30e. Screens (HTML) built live

A screen is a live web view, so the build can be played over the real page:

1. After each `canvas_screen_write`, take a **blueprint** of the rendered page inside the
   screen's web view: boxes with their fills, borders, radii and shadows; text blocks with
   their computed styles; images and SVGs; and repeated siblings grouped as master +
   duplicates. Diff it against the screen's previous blueprint, so only what changed is
   played.
2. Play it with the same engine and the same animations (30c, 30d), as marks over the screen's
   frame on the canvas. Then **fade from the masked drawing to the live web view**.
   That fade is the "go live" moment. The screen is interactive at once if the user clicks it.
3. Write this script ourselves, small and dependency-free. It runs only in Sawit's own web
   views. (Don't port `swarm.js`, see the licence note.)

#### 30f. Settings, sound, accessibility and cost

1. **Settings** (canvas menu and Settings): build animation **Full / Fast (2×) / Off**, Sound
   **off by default**, and "Show subagents as separate cursors" on by default. Add an MCP
   setting for it too (`canvas_command` or app settings), so an agent can turn it down for
   long batch jobs.
2. **Reduced motion:** the pointers jump, there is no drag or typing, and elements appear
   with a short fade only. **Screen readers:** keep the polite status line from
   `presence/announce.rs` (one sentence per change, per lane), never one per animated
   element.
3. **Sound** (if on): short synthesised clicks for clicks, typing and snaps, quiet, panned to
   the cursor, made at runtime (no audio files, no new heavy dependency). Our own sounds, not
   copies.
4. **Cost:** animate only what is on screen, sleep the animation loop when no lane is
   playing, and keep idle CPU at 0. With 6 lanes playing on a large canvas, stay at 60 fps on
   an M1 and report the CPU and memory before/after for item 7. Add no dependency without
   saying its size.

#### 30g. Done when

- Four subagents (30a) building four components show four cursors in four colours at once,
  each dragging out frames, typing text, duplicating cards and drawing connectors in their own
  component, with nothing overlapping and the frame rate held.
- A screen written in three `canvas_screen_write` calls plays three builds of only what
  changed and ends on the live, clickable page.
- The document is always ahead of the animation: `canvas_get`, export, save and undo mid-play
  all see the final state, and the user's selection, view and keyboard never change.
- Under reduced motion and with the setting on Off, nothing drags or types; with Fast, total
  play time per op is half.
- Tests: the scheduler's ordering (parent → child, master → duplicate, reading order), the
  catch-up budget, 4 lanes from one session, an undo that cancels a replay, and the blueprint
  diff for screens. Record a short screen capture of each case for the report.

---

## Suggested order

1. Bugs that block daily use: **11** + **18** (keyboard focus and ownership, done together),
   **16** (device preview text), then **15** (popups; case B depends on 16).
2. Safety: **13** (confirmations), **12** (close project closes terminals).
3. Terminal and layout: **2**, **1**, **22** (right-click menus, which use 1, 2 and 17),
   **17** (full-screen hotkeys, after 18 so the keys work inside web views), **21** (⌘F find bar
   in every view, also after 18).
4. Popup audit: the rest of **15**.
5. Browser and canvas: **6**, **10**.
6. GitHub: **19** (the sign-in must survive a restart; it comes first, because 5, 8 and 20
   depend on it), then **20** (shell files: one button and sync).
7. Tools and integrations: **8** (GitHub CLI), **3**, **23** (Herdr), **24** (HyperFrames),
   **29** (voxl), **4** and **28** (skills), **5** (5 can reuse the sync design from 20).
8. Bigger pieces: **14** (permissions page), **7** (size and memory, best after **10** so the
   tabs' memory is measured too).
9. **9** only after the user answers the questions above.
10. Canvas to code: **27** (design standard on by default) first, so every screen built for
    25 and 26 already follows it, then **25** (export buttons on artboards), then **26**
    (handoff for artboards, which reuses 25's export).
11. Live build cursors: **30a** (one cursor per real agent and subagent) first, as it is a bug
    and the rest depends on lanes, then **30b–30d** (cursor art, replay engine, element
    animations), then **30e** (screens, after 25–27), then **30f**. Do it after **18**, so the
    marks never take the user's keyboard, and measure it for **7**.

## Final checklist

- [ ] Each item is in its own commit and has a test where it can have one.
- [ ] `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` and the e2e specs pass.
- [ ] New features have matching MCP tools, and the README's MCP table lists them.
- [ ] Every new destructive action asks first, and every new popover fits its content.
- [ ] The popup audit list (item 15) and the focus-change table (item 18) are in the report.
- [ ] Every screen and artboard can be exported as HTML and React and handed to an agent
      (items 25, 26), and an agent that never calls `canvas_design_guide` still gets the
      design standard (item 27).
- [ ] Several agents and subagents show as several cursors at once, every element type plays
      its build animation, and the document is never behind the animation (item 30).
- [ ] The report lists what was done, how it was checked, before/after numbers for item 7, and
      open questions (item 9).
