# Backlog report, part 1: keyboard and safety (items 11, 18, 13, 12)

Back to the [summary](../2026-11-06-backlog-report.md).

---

## Items 11 + 18: keyboard focus and keyboard ownership

**Commit:** `bdca7d5` (fix(app): the user's keys go where the user is…). It was committed on `main`
before the backlog branch, so it is the base of `bdca7d5..backlog`, not inside that range.
An earlier canvas fix, `e87c540` (agent canvas ops no longer take the user's tab, project or
view), is also on `main`.

### Reproduced (before the fix)

`e2e/specs/focus.test.js`, run against the unmodified v0.0.9 build:

1. Type the fixture site's address and press Return (the page gets the keyboard). Press ⌘T (a new
   terminal, as the user). Type `echo focus-0` + Return: it works, because the page has no focused
   element and WebKit passes the keys up to GPUI. Press **Tab**: the page focuses its text field.
   From then on every key the user types "in the terminal" lands in the page (`echo focus-1` was
   typed into the page's field).
2. With the user typing in a terminal, an agent's `browser_type` focuses a field in the page by
   script. WebKit makes the page AppKit's first responder on its own, and the user's next line
   (`echo after-type`) lands in the page's field.

Cause: AppKit's first responder (which gets the keys) and GPUI's focus handle drifted apart.
⌘T, a click on the terminal or ⌘1 moved GPUI's focus, but a `WKWebView` that was first responder
stayed it. WebKit also takes first responder whenever the page focuses an element.

### Fix

- **One owner:** `crates/app/src/browser/engine/keyboard.rs`. Every web view (browser tab, canvas
  frame, screen preview) registers when it is built. A page may be AppKit's first responder only
  while GPUI's keyboard is on the view that hosts it (`Engine::set_keyboard_host`), or after the
  user's own mouse press in it (until GPUI's keyboard moves). Never while hidden, windowless or
  off screen. The rules are a pure function (`verdict`), unit-tested.
- **Enforced at three points:** on every workspace render, on every non-noise message from a page
  (and again 150 ms later), and in a local `NSEvent` key-down monitor that runs before AppKit
  delivers the key, so no key is lost between two frames. A mouse-down monitor tells the user's
  presses in a page apart from script focus.
- **Hidden views never hold the keyboard:** `Engine::set_visible(false)` gives it back first.
- **Agents never give a page the keyboard:** an agent's canvas `web enter` goes live without the
  keyboard. A live frame coming back from under a menu or a pan takes the keyboard again only if
  the user entered it and the canvas still has GPUI's keyboard. `Engine::focus` no longer panics
  for a view in no window.
- **Shortcuts inside a page:** ⌘ and ⌃ keys (⌃Tab, ⇧⌘↩) already reach GPUI first as key
  equivalents. A forwarding layer was written, measured as unnecessary (the ⌃Tab test passes
  without it) and deleted.
- **Focus is shown:** the center area and the terminal dock each draw a 2 px line in the theme's
  `ring` colour along their top while they hold the keyboard.

### Checked

- Unit: `browser::tests::keyboard::appkits_keyboard_follows_gpuis`, plus the 8 existing keyboard
  tests.
- e2e `focus.test.js` (real app, real key events through agent-desktop):
  1. after the page had the keyboard, the terminal the user opens gets every key (3 rounds, with
     Tab);
  2. while the user types in a terminal, an agent runs navigate, type, click, press,
     `terminal_run`, opens the canvas, lays out a route frame, enters it live and reopens the
     browser: the terminal gets every user key, the page none;
  3. twenty rounds page ↔ terminal: every key lands where the user is;
  4. ⌃Tab switches tabs while the page has the keyboard;
  5. the user typing in the page keeps every key, in order, while an agent works in terminals.
- Later in the backlog, item 10 changed the spec: ⌘T in the browser now opens a browser tab, so the
  spec opens its terminal with the dock's New Terminal button.
- `focus: twenty rounds…` is slow (229 s) and flaky when the window loses key status or is not
  exposed through accessibility. It passed on rerun in the fix-ui track.
- **Not done literally:** the item's "agent loop for 2 minutes" was not run as a 2-minute loop.
  The spec runs one scripted agent mix per test.

### Focus-change audit (item 18, step 1)

Every place in `crates/app/src` and `vendor/gpui-base/src` that moved GPUI's focus or AppKit's
first responder at the start of the backlog (≈220 call sites, tests excluded), by who triggers it:

| Where | Calls | Triggered by | Allowed? |
|---|---|---|---|
| `vendor/gpui-base` (input, questionnaire, dock, text, menus…) | 72 | the user's click or key inside the widget | yes |
| `keymap/editor`, `mcp_page`, `skills_page`, `tools_page`, `new_project_page`, `shell_config`, `db/*`, `git/*`, `files`, `editor`, `viewers`, `run/panel` | 49 | the user (clicks, keys). Pages opened by an agent pass `focus: false` (`show_keymap_editor`, e87c540) | yes |
| `terminal/panel`, `terminal/tab`, `terminal/view` | 19 | the user (`Origin::User`). Agents go through `reveal(Origin::Agent)`, which shows without focus and never while the user types in a terminal. A closed tab hands on the keyboard only if it had it | yes |
| `workspace/keyboard.rs` (`KeyboardHome`) | 3 | the system: the keyboard leaves what is no longer drawn, and comes back to it | yes (it only gives back) |
| `workspace/areas.rs` | 2 | the user (welcome screen's Open Terminal; ⇧⌘↩ with `Origin::User`). The agent's `maximize_terminal` passes `Origin::Agent` | yes |
| `workspace/project` (`give_keyboard`) | 2 | the user's click in the Files tab | yes |
| `browser/mod.rs` `give_page_keyboard` | 1 | Return in the address field (user) | yes |
| `browser/events.rs` `Pressed` | 1 | the user's press in the page | yes |
| `browser/*` `focus_parent` (address focus, notes, panel, viewport hide, drop) | 6 | the user, or hiding | yes (gives back only) |
| `board/web/live.rs` un-hide `engine.focus()` | 1 | **a re-render after a menu or pan, including after an agent's op** | **was not** → now only if the user entered the frame and the canvas has GPUI's keyboard |
| `board/web/live.rs` `web_enter` `engine.focus()` | 1 | the user (double-click, Enter, Live) **or an agent's `web enter` op** | **was not** → agents enter without the keyboard |
| `board/screens/preview.rs` first placement | 1 | the user's Preview click | yes |
| WebKit itself (script `focus()`, autofocus) | — | **a page, or an agent's `browser_type`** | **was not** → given back by the owner unless the user pressed there |

Focus paths added later in the backlog, as their tracks report them. This is not a second full
audit.

| Added by | Behaviour |
|---|---|
| Item 10, browser tabs | An agent's `browser_*` call on a tab not on show wakes it hidden, never key and without a keyboard host. The user's tab and keyboard stay put. |
| Item 13, shared Confirm | The dialog's Cancel takes the keyboard while it is open, and the dialog layer puts it back where it was when it closes. Agents' control-socket paths never open it. |
| Item 17, full screen | ⇧⌘↩ fills the panel that has the keyboard, and the keyboard stays inside it (checked with the page holding the keyboard). |
| Item 21, find bar | Escape or ✕ gives the keyboard back to the view the bar searched. The terminal ignores keys while it doesn't hold the keyboard. |
| Item 30, cursors | Agents' cursors are painted marks only. The e2e checks that the user's selection and view don't change. |

---

## Item 13: every destructive action asks first

**Asked:** list every action that removes, closes, stops, overwrites or discards, and make each one
ask with one shared confirm dialog: it names the thing, has a red button with a specific verb,
Cancel is the default and Escape cancels. The worst actions make the user type the name.

**Built:** `crates/app/src/confirm/mod.rs` (`Confirm`), on gpui-component's window dialog layer,
not the native `window.prompt` (NSAlert has no red button and no text field, and its first button
takes Return). One builder: `.thing()` (bold in the title), `.detail()`, `.items()` (the first 6,
then "and N more"), `.second()` (a second red button), `.type_to_confirm()`, `.ask()`.

- Cancel has the keyboard when it opens. Return answers Cancel, Escape cancels, a click on the
  backdrop cancels. Only a click on the red button (or Tab to it, then Space) goes ahead.
- With `type_to_confirm`, the red button stays off until the typed name matches.
- The dialog covers the whole window, so the browser page and canvas web views step back while it
  is up (`native_cover`, 836be2b).

**Commits:** `e23e8f1` (item 13), `ed75cc9` (follow-up: the other tracks' destructive actions).

### Inventory (paths under `crates/app/src/`)

"prompt" is the native `window.prompt` alert. "Confirm" is the new shared component.

| Area | Action | File | Before | After |
|---|---|---|---|---|
| db | Delete table, view or matview (and "with Dependents") | db/panel/jobs.rs | prompt | Confirm "Delete Table" + second red "Delete Table with Dependents"; remote DB: type the name |
| db | Empty table | db/panel/jobs.rs | prompt | Confirm "Empty Table"; remote: type the name |
| db | Delete or update rows | db/table/selection.rs | prompt | Confirm "Delete Rows" / "Delete All Rows" / "Update Rows"; whole table or remote: type the name |
| db | Run SQL that writes (SQLite, D1) | db/sql.rs | prompt "Run" | Confirm "Run Changes"; remote D1: type the name |
| db | Remove saved connection (and its Keychain password) | db/panel/settings.rs | inline "Confirm Remove" | Confirm "Remove Connection" |
| db | Drop database or schema | — | MCP only | MCP `drop_database` keeps its `confirm` flag |
| instances | Stop instance | servers/detail/instance.rs | none | Confirm "Stop Instance" |
| instances | Remove from list (keeps data dir) | servers/detail/instance.rs | none | Confirm "Remove Instance" |
| instances | Overwrite DATABASE_URL in .env | servers/detail/instance.rs | none | Confirm "Replace DATABASE_URL" when a different one is there |
| instances | Restart | servers/detail/instance.rs | none | unchanged (comes straight back) |
| ssh | Delete saved server (and its Keychain password) | ssh/panel/host.rs | none | Confirm "Delete Server" |
| terminal | Close a tab with a running process | terminal/panel/mod.rs | none | Confirm "Close and Stop", naming what runs |
| terminal | Close a pane with a running process | terminal/tab/panes.rs | none | Confirm "Close and Stop" |
| terminal | Close an idle terminal | same | none | unchanged: no dialog, as the item says |
| terminal | Quit, Restart to Update or Close Window while terminals run | terminal/guard/mod.rs | prompt | Confirm "Quit and Stop" / "Restart and Stop" / "Close and Stop" |
| workspace | Close a file with unsaved changes | workspace/close.rs | Save / Don't Save / Cancel | unchanged (not a destructive question) |
| workspace | Close the browser with unsent annotations | workspace/close.rs | prompt | Confirm "Close and Discard Notes" |
| workspace | Close Dev Tools or Skills while installing | workspace/pages.rs | prompt | Confirm |
| workspace | Close New Project while it creates | workspace/new_project.rs | prompt | Confirm |
| workspace | Close a project with unsaved files, notes or running terminals | workspace/project/mod.rs | prompt | Confirm (item 12 adds the terminals) |
| workspace | Close Others / Left / Right / Clean, Safe Close All | workspace/tab_bar.rs | none | unchanged: they skip pinned and unsaved tabs |
| workspace | Force Close All (new) | workspace/tab_bar.rs | — | Confirm naming the files whose changes it discards |
| files | Move to Trash in Files and Config Files | files/mod.rs | prompt | Confirm "Move to Trash" (undo stays) |
| files | Delete permanently | files/commands.rs, shell_config/commands.rs | prompt | Confirm "Delete" |
| files | ⌘⌫ to Trash | files/render.rs | undo | unchanged (undoable) |
| git | Discard changes, delete untracked file | git/panel/files.rs | prompt | Confirm "Discard Changes" / "Delete File" |
| git | Sign out of GitHub | git/account/controls.rs | prompt | Confirm "Sign Out" |
| git | Merge branch or PR; close PR or issue | git/* | prompt / none | unchanged (not destructive, or reopenable) |
| browser | Clear annotations | browser/annotate/rows.rs | prompt + undo | Confirm + undo |
| browser | Delete one annotation | browser/annotate/* | undo toast | unchanged |
| browser | Close a browser tab with unsent notes (item 10) | browser/tabs/mod.rs | prompt | Confirm "Close and Discard Notes" |
| canvas | Delete page, layers or effects | board/* | undo | unchanged (undo) |
| canvas | Delete Canvas… (item 10) | workspace/canvas_tabs.rs | prompt | Confirm "Delete Canvas"; the file goes to the Trash |
| catalog | Uninstall a dev tool (Herdr included) | tools_page/queue.rs | prompt | Confirm "Uninstall" |
| catalog | Delete Settings… (item 23) | tools_page/extras.rs | prompt | Confirm "Delete Settings", listing the paths |
| catalog | Remove a skill pack / Sawit Skills | skills_page/jobs.rs, sawit_skills/mod.rs | prompt | Confirm "Remove Skills" / "Remove Sawit Skills" |
| catalog | Uninstall or remove an MCP server | mcp_page/install.rs | prompt | Confirm "Uninstall" / "Remove" |
| catalog | Stop a running install | tools, skills, MCP pages | none | unchanged (re-runnable) |
| ports | Stop one or N jobs (SIGTERM) | ports/row.rs, ports/render.rs | none | Confirm "Stop Process(es)", listing each with its ports |
| ports | Force Stop (SIGKILL) | ports/render.rs | none | Confirm "Force Stop", listing the PIDs |
| run | Delete a saved command | run/panel/editor.rs | none | Confirm "Delete Command" |
| run | Stop or restart a script | run/panel | none | unchanged (one click restarts it) |
| prompts | Delete a prompt | prompts/view.rs | prompt | Confirm "Delete Prompt" |
| prompts | Restore prompts from GitHub (item 5) | prompts/github.rs | prompt | Confirm "Restore Prompts" |
| shell files | Upload N files / Replace N local files (item 20) | shell_config/sync.rs | prompt | Confirm "Upload N Files" / "Replace N Files" |
| permissions | Save a permission change (item 14) | permissions_page/render.rs | diff sheet | diff sheet kept (it is the confirmation, with a backup); "Save Change"; Return never presses it |
| keymap | Remove a binding of the user's own | keymap/editor/mod.rs | none | Confirm "Remove Binding" |
| accounts | Remove an account, end a session, Remove Claude Pool (item 9) | the Accounts page | — | Confirm |
| welcome | Remove from Recent | welcome.rs | none | unchanged (a list entry) |

Still native, because they are not destructive: opening an external link, Merge, OK notices,
"Save Anyway" for secrets sent to GitHub, Save / Don't Save for unsaved files. Item 9's settings
changes that keep a backup and can be undone (tracking on, the shell block, the import) also use
the system alert.

**MCP:** every destroying tool is in `crates/mcp/src/hints.rs` `DESTRUCTIVE` (MCP's
`destructiveHint`), and the README says which need an explicit `confirm`. Agents' calls never open
the dialog: the agent's own permission layer is in charge, and a dialog would take the user's
keyboard.

**Decisions:**
- Return means Cancel everywhere in the dialog, even when the red button is focused. Space presses
  the focused button.
- Type-the-name is for writes and deletes on a remote (production) database and for deleting every
  row of a table. Sawit's UI has no "drop database" or "delete instance data" action.
- Undo instead of a dialog where undo already exists or the action is one click to redo.

**Checked:** `confirm::tests` (7: Return cancels and gives the keyboard back, Escape cancels, red
button by Tab and Space, typed-name gating, long lists cut) and the converted prompt tests through
`confirm::testing`. e2e `confirm: closing a terminal that runs something asks first, and Return
cancels` passed (safety-e2e-1 and -3).

---

## Item 12: closing a project closes its terminals

**Asked:** closing a project closes its terminals, stops their processes cleanly and leaves no
orphan processes or ports. Ask first if something runs.

**Built (`92ecd36`):**
- Closing a project (title bar ×, ⌃⌘W) closes every terminal tab grouped with it, Run panel scripts
  included. Tabs of no project stay.
- If one runs something, a file is unsaved or the browser has unsent notes, the shared Confirm asks
  and lists what would stop ("Close web and stop 2 running processes?", "Close and Stop").
- Every terminal that goes reads its shell's process tree (`proc_listchildpids`) while the shell is
  still the parent. Then a thread sends SIGHUP, SIGTERM after 0.8 s and SIGKILL after 2.5 s
  (`sawit_core::terminal::reap`). Quit and Restart to Update do the same with short, waited-for
  grace periods.

**Checked:** `reap` unit tests (tree walk; a tree that ignores HUP and TERM still ends); a panel test
(closing a project kills a stubborn background job and a foreground one, and keeps the no-project
tab); e2e `project close: its terminals close, and the server they started stops` (an `nc` on a
port; after Close and Stop the port is free and the process gone) passed. The test used `nc`, not
`bun dev`.
