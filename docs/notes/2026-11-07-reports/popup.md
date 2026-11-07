# Track `popup` report (B16, B13, B5)

**B16 shared component commit: `3435673`** (branch `wt/popup`): the menu look in `vendor/gpui-component/src/menu/popup_menu.rs`, `crates/app/src/menu_icons.rs`, `crates/app/src/board/float_bar.rs`. Ready to merge early for B14/B15.

Worktree `/Volumes/bahrul/project/sawit-wt/popup`, branch `wt/popup`.

## Status

B16 done, B13 done, B5 done (⇧Tab tab switching deliberately not bound, see Decisions). Final tree: `cargo test -p sawit` 886 passed, `-p sawit-board` 821 passed, clippy clean, fmt clean; e2e reruns (keymap, menus-look, popups, handoff, fullscreen, focus, tabs, browser, terminal, copy-output, editor) pass.

## Commits (in order)

| # | Hash | Item | What |
|---|---|---|---|
| 1 | `3435673` | B16 | shared parts: menu look (vendor patch), `menu_icons`, `float_bar`; route bar, timer/vote pill, Preview bar, context bar on them; menu tests; design file |
| 2 | `6ac8e1d` | B16 | sweep: icons on every menu item, danger items, canvas engine menu icons, the list of menus and bars (`docs/notes/2026-11-07-b16-menus-and-bars.md`) |
| 3 | `b07e49c` | B13 | screen, artboard and flow bars on `FloatBar`; Send to Agent menu |
| 4 | `9880f03` | B13 | e2e: the screen bar is pictured without the accessibility snapshot (it times out over a live screen) |
| 5 | `6027a8d` | B5 | area keys, ⌘N, ⇧⌘N, ⌥Tab, clashes resolved, menu bar, README |

Commit 1 was checked on its own (exported tree, built and tested); 2 and 3 are layered on the
tree the full builds and test runs used.

## Decisions

- Sawit ships one theme (Zed One Dark, `crates/app/src/theme.rs`), so the design and the reviews
  are dark only; "light and dark" doesn't apply until a light theme exists.
- `canvas_export` of the design screen came out blank (the screen's web view isn't captured,
  likely B17); the design HTML was checked by rendering it with Quick Look.
- A checked item shows its check in the icon's slot instead of its icon (the state wins), so a
  choice list keeps one column.
- The "every item has an icon" test is a source scan, like the assets test: a
  `PopupMenuItem::new/link/submenu/element` or `menus::keyed` call must chain `.icon(…)`,
  `.leading(…)` (a logo) or `.checked(…)`, or carry `// no icon: <why>` on the 3 lines above;
  the iconless builders (`.menu(`, `.submenu(`, `.menu_with_disabled(`…) fail in files that
  build menus.
- Floating bar widths come from the labels' shaped text (`FloatBar::measure`), so the bar is
  placed, centred and clamped on its first frame and tests can check there's no blank space.
  On a narrow canvas all labels collapse to icons at once.
- Export on the screen bar is one menu: HTML, React, PNG (the screen's latest picture, asking
  where). An artboard also has SVG (PNG and SVG go through File ▸ Export, to the exports
  folder). No SVG for a screen.
- File tree: "Trash" reads "Move to Trash" and moved with Delete to the last group, both in the
  danger colour; both still ask as before.
- Preview's row of size buttons became one Size menu (device icons, the size checked).
- The timer/vote pill uses the same bar but isn't moved out of the way of frame bars at the top
  edge (the backlog defers "shows only while used" to B15).
- Accounts files only got `.icon(…)`/`.danger(…)` additions, to keep B14's merge small.
- B5: a place's own key wins while it has the keyboard. ⇧⌘C stays the canvas's Copy as PNG
  (FigCraft's) and the terminal's Copy Last Output; from the canvas, ⌃⌘3, ⇧⌘↩ or the Maximize
  button fill the window. ⌘T stays the browser's New Tab while the browser has the keyboard.
  ⌘J is unbound but still kept from the canvas (so Join keeps ⌃J and a user binding works there).
- B5: ⇧Tab isn't bound to tab switching anywhere: no Sawit tab bar takes the keyboard, so there's
  no place where "nothing else uses it"; it stays the terminal's and the programs'. ⌃Tab/⌃⇧Tab
  switch tabs.
- B5: ⌘T is the terminal's everywhere, the browser included (the backlog: pages don't get ⌘T), so
  the browser's New Tab moved from ⌘T to ⌃T (its + button is unchanged).
- B5: no new MCP tool: agents already reach the same actions through `app_layout`,
  `app_show_panel`, `app_open_page` and `app_maximize`, and only the user's keys move focus.
- B5: ⌘B/⌘U hiding the browser/canvas shows the tab before it (after it, for the first); with no
  other tab, the tab area hides.

## B16

Changed: commits 1–2. Tests: `menu_tests` (28 px rows and label alignment,
`every_menu_item_has_an_icon`, `an_item_without_an_icon_has_no_picture`), `float_bar` units,
`board::menus::icons`, the assets test; full `cargo test -p sawit` (886 passed) and
`-p sawit-board` (821 passed) on the final tree. e2e: `menus-look.test.js` (route bar with a
`switch` role and its checked state, canvas main menu, file tree and terminal tab menus),
`popups.test.js`, `handoff.test.js` pass.

## B13

Changed: commit 3. Tests: `board::screens::tests::bars` (artboard and flow bars fit their
buttons, collapse to 28 px icons on a 150 px canvas, the Send menu in its three states);
`handoff.test.js`; `menus-look` B13 test.

## B5

Changed: commit 5. Keys (Workspace): ⌘T terminal, ⇧⌘T fill with it, ⌥⌘T new terminal; ⌘U
canvas, ⇧⌘C fill with it; ⌘B browser, ⇧⌘B fill; ⌘E folder panel; ⌘N new file
(`NewFileInProject`, the tree's New File from anywhere); ⇧⌘N New Project; ⌥⌘N New Window;
⌥Tab/⌥⇧Tab terminal ↔ tabs; ⌘K O Close Others; ⌘J and ⌃⌘N unbound. Canvas policy lets the
area keys through (`SAWIT_WINS`), ⌘U stays underline while text is edited. Menu bar: File ▸ New
File / New Terminal; View ▸ Folder Panel, Terminal, Browser, Canvas, Next/Previous Panel.
README shortcut table updated. Users' own keymap bindings are untouched (they override defaults
as before).

Tests: `workspace::tests` (`the_area_keys_open_close_and_fill`, `no_default_binding_takes_a_macos_key`,
`no_two_shortcuts_share_a_keystroke` with ⌥⌘N), `keys_tests::the_area_keys_lose_only_to_their_own_places`
(terminal, file tree, browser, keymap editor, canvas contexts), board keymap tests, terminal
key tests (⌥Tab is the app's; the pane menu shows ⇧⌘T for Full Screen), sawit-board shortcut
tests; e2e `keymap.test.js` presses ⌘E, ⌘U, ⌘B, ⇧⌘B, ⌘T (from the browser) and ⇧⌘N and checks
the layout and tabs; `fullscreen`, `focus`, `tabs`, `browser` specs pass (two harness flakes,
"unexpected reply" from the control socket, passed on rerun).

## Design review (better-interface), B16 + B13, rendered build (dark)

Scope: every menu and canvas floating bar touched, from e2e screenshots of the debug build
(main menu, file tree menu, terminal tab menu, Send to Agent with no agent, Export, route bar,
artboard bar, timer pill, browser device menu); narrow canvas checked by unit test (150 px), not
by screenshot. Project docs read: backlog ground rules, `vendor/gpui-component/SAWIT_PATCH.md`.

| Severity | Location | Finding | Status |
|---|---|---|---|
| MEDIUM | `popup_menu.rs` `render_icon` | Disabled items kept full-strength icons: the state showed on the label only | Fixed: the icon dims with the label |
| MEDIUM | `files/render.rs` file menu | Collapse All's `ChevronsDownUp` read as a close "×" at 14 px | Fixed: Unfold/FoldVertical |
| MEDIUM | `board/screens/preview.rs` | Title cut short in a narrow canvas with no way to the full text | Fixed: tooltip with the whole title |
| MEDIUM | `board/routes/bar.rs` | Full Page's state carried by a background only | Fixed: switch role + checked state, accent icon and check (e2e asserts the state) |
| LOW | `board/facilitation.rs` | The timer pill can sit under a frame bar at the canvas's top edge | Open, B15 (the backlog defers it) |
| LOW | menus | Menu text is the app's 11.4 px `text_sm` (13 px rem) | Project density, kept |
| LOW | Send to Agent menu | "Get a Coding Agent…" opens a page, not a dialog, yet has an ellipsis | Kept: the backlog's wording |

Domains: accessibility (icon-only buttons named by their labels, toggles as switches, menu items
keep text labels; keyboard paths unchanged), layout (no blank space: unit tests; centred and
clamped), writing (verb first, no ellipsis on menu buttons), typography (tabular figures on the
timer), colours (primary text #282c33 on #74ade8 ≈ 7.5:1; muted icons #a9afbc on #2f343e ≈ 6.4:1;
danger only on destructive items), UI (bar radius 10 = 6 + 4).
**Verdict: Approve** (no HIGH; the MEDIUMs are fixed).

## Not verified

- B5: a WebKit page with the keyboard: whether ⌘U/⌘E/⌘B/⌘N reach Sawit before the page (the
  e2e pressed them with Sawit's side focused). ⌥Tab inside programs that use Option as Meta in
  the terminal: they lose ⌥Tab (by design, per the backlog).
- B5: ⌘N's new file in the folder selected in the tree is the tree's own logic; e2e didn't type a
  name after ⌘N from outside the tree.

- VoiceOver reading of the new menus (only the AX tree through the e2e harness).
- The collapsed (icons-only) bar in a real narrow window: unit-tested only.
- Light theme: none exists.
