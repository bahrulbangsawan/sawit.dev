# Backlog report, part 2: terminal, full screen and find (items 2, 1, 22, 17, 21)

Back to the [summary](../2026-11-06-backlog-report.md).

---

## Item 2: lock, pin and close-all for terminals

**Asked:** a locked tab can't be closed by anyone until unlocked; a pinned tab moves to the start,
looks compact and comes back pinned; Close All / Close Others skip locked tabs and ask first.

**Built (`ead212b`):**
- **Lock:** a lock icon replaces the ×. The tab refuses ×, menu Close, ⌥⌘W, Close Others/All, and
  `terminal_close` (a clear error, even with `confirm`). A note over the tabs says why. Typing stays
  allowed.
- **Pin:** the tab moves to the pinned run at the strip's start, compact (name up to 80 px plus a
  pin icon), and stays put on regroup. Pins are kept per project in `<data>/terminal-pins.json`
  (name, folder, lock).
- **Close Others / Close All** keep locked and pinned tabs, say "Kept N tabs (locked, pinned or
  busy).", and ask first, naming what runs ("Close 2 terminals and stop `sleep`?").
- **MCP:** `terminal_lock`, `terminal_pin`; `app_state` terminals carry `locked` and `pinned`.
  README table and terminal section.

**Decision:** terminals don't survive a restart, so a pinned tab comes back as a fresh shell in its
folder, with its name and lock. The pin survives, not the process.

**Checked:** panel tests (lock refuses user and agent closes; pin moves to the start and bulk closes
keep it; Close All asks before stopping; pins come back with their project), a control round-trip
test, sawit-mcp tests. e2e `terminal lock: …` passed (safety-e2e-5).

**Not done:** Sawit has no command palette, so "from the command palette" is covered by keymap
actions (bindable in the Keymap Editor) instead.

---

## Item 1: copy error logs from the terminal and from instances

**Asked:** Copy Last Output / Copy Error in the terminal menus; Copy Logs / Copy Errors on an
instance; plain text, original order, a toast.

**Built (`991f4de`):**
- **Terminal:** Copy Last Output (⇧⌘C) and Copy Error (⌥⇧⌘C) in the pane and tab menus.
  - No shell integration exists (alacritty doesn't keep OSC 133 marks). So the pane remembers the
    cursor row when Return reaches the shell at its prompt. The output is the rows after it, up to
    the next prompt (or the end while running). With no command seen, it copies the visible screen.
  - Plain text by construction (the grid holds characters), wrapped rows joined.
  - Copy Error keeps error lines ± 2 lines, with "…" between runs. Toast: "Copied 12 lines,
    1 error".
- **Instances:** Copy Logs (the last `LOG_LINES` = 200, `sawit_core::copy_logs`) and Copy Errors
  (ERROR/FATAL/PANIC ± 2 lines), ANSI stripped, a "Copied 200 lines" note for 3 s.
- **MCP:** none added. Agents already have `terminal_read` and `instance_logs`; this copy is for the
  clipboard.

**Checked:** `copy_logs` unit tests, a real shell's coloured output copied as plain text, the
instance log tail and error tests, pane menu key tests. e2e `copy-output.test.js` reads the
clipboard with `pbpaste`. It failed in the integrator's 72/13 run because the spec typed keys the
harness can't press and a command that was too long. The spec was fixed in `50f7492` and `7b3e66f`.
Its pass is part of the final run (see the summary).

---

## Item 22: the terminal's right-click menus, plus Safe and Force Close All

**Asked:** both terminal menus as complete as the file tab menu, with shortcuts from the keymap,
disabled (not hidden) items, Title Case, and closes that respect locks and ask before stopping
processes. The user also asked for **Safe Close All** and **Force Close All** in the terminal and
file tab menus.

**Built (`07d426b`, wired to items 17 and 21 in `ed75cc9`):**
- **Tab menu** (`terminal/panel/tab_menu.rs`): Rename Tab, Duplicate Tab, Move to Project ▸ (names
  cut at 40 characters with "…"; "No Other Project Open" when none), Remove from Group | Close,
  Close Others | Close Left, Close Right | Safe Close All, Force Close All | Lock/Unlock Tab,
  Pin/Unpin Tab | Copy Last Output, Copy Error, Clear, Export Output… | Full Screen.
- **Pane menu** (`terminal/tab/menu.rs`): Copy, Paste, Select All, Find… | Copy Last Output, Copy
  Error, Open Link, Copy Link, Open File (on the token under the click; paths only if they exist) |
  Clear, Reset Terminal | Rename Pane, Split Right, Split Down, Zoom/Unzoom Pane, Full Screen | Send
  Right-Clicks to Pane ✓ | Close Pane.
- Disabled, not hidden: Close (locked), Close Others (alone), Close Left (first), Close Right
  (last), Move to Project and Remove from Group when there's nothing to do.
- Keys are read from the keymap. New Terminal-context defaults: ⌘V Paste, ⌘A Select All, ⌘K Clear
  (it waits a moment for the workspace's ⌘K chords), ⇧⌘C and ⌥⇧⌘C (item 1).
- **Find…** runs item 21's `find::Find` (⌘F). **Full Screen** runs item 17's
  `workspace::MaximizeTerminal` (⌃⌘1).
- **File tab menu:** Safe Close All (⌘K W, the old Close All: keeps pinned tabs and tabs that would
  lose something, and says how many it kept) and Force Close All (⌘K ⇧W: every tab, pinned
  included, after a Confirm naming the files whose changes it discards). Also in the File menu.
- **MCP:** `terminal_close` takes `scope` (others/left/right/idle/all; it refuses without `confirm`
  while a tab it closes runs something; locked tabs are always kept), `terminal_clear`,
  `app_close_all_tabs {force, confirm}`.

**Decisions:**
- For terminals, Safe Close All is exactly the item's "Close Idle" (closes tabs with nothing
  running, keeps locked and pinned ones), so the menu has two items, not three. The MCP scope keeps
  `idle`. Force Close All closes every tab except locked ones, after naming what stops.
- ⌘W and ⌥⌘T in a focused terminal keep closing center tabs, as the existing design in
  `terminal/view/keys.rs` says. So Close and Close Others in the terminal tab menu show no key. The
  actions exist (`terminal::CloseTab`, …) and can be bound.
- ⌘C stays in the terminal's `on_key` (a test forbids any binding from taking ⌘C from programs), so
  the pane menu's Copy shows no key.
- No command palette exists. A menu-bar "Terminal" menu was tried as a stand-in and removed, because
  the app's test "menu keys hold everywhere in a window" forbids menu items whose keys only work in
  one view.

**Checked:** tests of the tab menu's shape and enabled items for the first, last, locked and busy
tab (`panel/tests/menus.rs`); the pane menu's shape and enabled items (`tab/tests/keys.rs`); keys
per item; Force Close All wording; scoped closes; link and path parsing. e2e
`terminal-menus.test.js` and the updated `terminal.test.js` (Title Case labels). The safety track's
own runs of these failed on a locked screen; they ran in the integrator's full runs.

---

## Item 17: full-screen hotkeys for the terminal, browser and canvas

**Asked:** a hotkey that makes the focused panel fill the window and the same key (or Escape) to
come back to the exact layout; direct keys per panel; keymap actions; a hint and an exit button;
MCP `app_maximize`.

**Built (`0c16005`; browser fix `bd148be` after item 10, and `2d17d38` in the fix-ui pass):**
- `workspace/fill.rs` generalises the terminal and canvas maximize into one "fill": the terminal or
  the browser/canvas tab fills the window; the sidebar, the other area and the status bar hide.
  The layout to restore survives going straight from one filled panel to another. Split sizes are
  now kept in Workspace-owned `ResizableState`s, so they come back exactly.
- Keys (rebindable, in `keymap_list`): ⇧⌘↩ `workspace::ToggleMaximizePane` (the panel with the
  keyboard); ⌃⌘1 / ⌃⌘2 / ⌃⌘3 for terminal, browser and canvas.
- Escape `workspace::RestoreLayout` works only in the browser (outside the page). The terminal and
  canvas keep their own Escape.
- While filled: "⇧⌘↩ to exit" in the title bar for 3 s and a Restore Layout button. It is in the
  title bar, not an overlay, because a GPUI overlay over the native page freezes it.
- Menus: View ▸ Maximize Panel / Terminal / Browser / Canvas in the macOS menu bar; the canvas main
  menu and Actions palette; the terminal's Maximize tooltip.
- MCP `app_maximize {panel}`, with `app_maximize_terminal` kept as an alias; `app_state.maximized`.

**Decisions:** ⌃⌘+letter are the pages, ⌘+digit the sidebar panels and ⌥1–⌥3 FigCraft's panels, so
⌃⌘+digit was the free set (a unit test asserts FigCraft binds none). macOS's own full screen stays
on ⌃⌘F.

**Fix during integration:** once browser tabs (item 10) put the browser inside `BrowserTabs`, ⇧⌘↩
filled with the wrong view, and the next render ended the fill and lost the layout. `bd148be` and
`2d17d38` fill with the workspace tab's view and read it back as the browser.

**Checked:** unit tests (fill settling, key clash); e2e `fullscreen.test.js` (4 tests: terminal
with typing still landing; browser while the page has the keyboard, which keeps it, and Escape from
the address field; canvas with Restore Layout; MCP chains). All pass, and the layout is compared
exactly before and after (fixui-e2e-2).

---

## Item 21: one ⌘F find bar everywhere

**Asked:** one shared find bar with count, previous/next, Match Case, Whole Word, Regex, Escape
giving the keyboard back; per view: editor (with Replace), Markdown, PDF, browser, canvas,
terminal; keymap actions.

**Built (`05f0700`; follow-ups `a5e5d1b`, `856b76b`, `e2e1166`, `eaeab03`):**
- `crates/app/src/find` (FindBar and the host helper) and `sawit_core::find` (regex matching,
  `FindState`). The bar shows "3 of 12", "No results" or "Invalid regular expression"; ↩/⇧↩,
  ⌘G/⇧⌘G; Match Case, Whole Word and Regex where the view supports them; All Pages on the canvas;
  Replace / Replace All (⌥⌘F) in editable files only. A selection seeds the query. It wraps in
  narrow panels.
- Keymap actions: `find::Find` ⌘F, `find::FindNext` ⌘G, `find::FindPrevious` ⇧⌘G, `find::Replace`
  ⌥⌘F, `find::CloseFind` (Escape in the bar). Edit ▸ Find submenu.
- **Editor:** the input's own engine, with its built-in panel turned off (`856b76b`). Match Case
  only (its matcher is Aho-Corasick). Replace is hidden while read-only.
- **Markdown:** the preview is now an editor-owned text view with highlights. In split view, the side
  with the keyboard is searched. HTML, SVG and diagram previews say "Search isn't available here".
- **PDF:** PDFKit's text layer through the ObjC runtime (a system framework, no new crate).
- **Browser:** JS in Sawit's isolated world plus the CSS Custom Highlight API (all matches, the
  current one stronger, a count, regex). Chosen over `WKWebView.find`, which marks one match and
  gives no count.
- **Canvas:** layer text and names, current page or All Pages (`sawit_board::layers::find`); each
  match is selected and zoomed to. ⌘G outside the bar stays Group.
- **Terminal:** scrollback plus screen, wrapped lines joined; highlights stay correct under new
  output; ⌘F never reaches the PTY; ⌃F stays the program's. Keys typed in the bar stay in the bar
  (`e2e1166`).
- Images, video and other tabs say "Search isn't available here".

**Checked:** unit tests (find state, terminal matching across wrapped lines and scrollback); e2e
`find.test.js` (code file, Markdown preview, PDF, browser page with the keyboard in the page,
canvas, terminal scrollback) passed (find-e2e-5: 23 pass, 0 fail).

**Not done / follow-ups:**
- The browser finds matches within one text node only (`foo<b>bar</b>` is not found as "foobar").
- Regex and Whole Word in the code editor need its matcher replaced.
- The terminal's current match can drift by a line when the 10,000-line scrollback is full and
  still scrolling.
- No MCP tool for find (not asked).
