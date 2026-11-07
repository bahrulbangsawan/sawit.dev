# B16: every popup menu and floating bar

The list the backlog asks for (B16, "Go through every menu and bar, one by one"): one row per
menu or bar, where it is, its items and the icon each gets. The icons come from one map,
`crates/app/src/menu_icons.rs`, so the same action has the same icon everywhere; the menu look
(28 px rows, 4 px padding, 14 px icons in a 16 px slot 8 px before the label, 1 px separators with
4 px either side, muted shortcuts, section headers, the danger colour) lives in the shared
component, `vendor/gpui-component/src/menu/popup_menu.rs`; the canvas's bars share
`crates/app/src/board/float_bar.rs`. `crate::menu_tests::every_menu_item_has_an_icon` fails on
a menu item with a label and no icon.

Design: `docs/design/b16-popups.sawit` (dark; Sawit ships one theme). Screenshots of the built
menus and bars: `e2e/specs/menus-look.test.js` and `e2e/specs/popups.test.js` write them to
`$SAWIT_E2E_SHOTS`. No "before" screenshots were kept: the backlog's 2026-11-07 screenshots are
the before.

## Menus

| Menu | Where | Items → icon |
|---|---|---|
| Account chip | `accounts/chip.rs` | accounts (check on the active one), Most Room Left · Switch → ArrowLeftRight, Accounts Page → Users |
| GitHub account / Sign In | `git/account/controls.rs` | header (the account's picture in the icon slot), Re-check → RefreshCw, Sign Out → LogOut, … (had icons) |
| File tab menu | `workspace/tab_bar.rs` | Close → X, Close Others → CircleX, Close Left/Right → ArrowLeftToLine/ArrowRightToLine, Close Clean and Safe Close All → CircleCheck, **Force Close All → CircleX, danger**, Make Tab Read-Only/Editable → Lock/LockOpen, Pin/Unpin Tab → Pin/PinOff; group: Close Tabs in Group → CircleX, Move to Project ▸ → MoveRight (projects → FolderOpen), Remove from Group → SquareMinus; canvas tab: New Canvas → Plus, Rename Canvas… → Pencil, **Delete Canvas… → Trash, danger** |
| Terminal tab menu | `terminal/panel/tab_menu.rs` | Rename → Pencil, Duplicate → CopyPlus, Move to Project ▸ → MoveRight, Remove from Group → SquareMinus, Close → X, Close Others/Left/Right, Safe Close All → CircleCheck, **Force Close All → CircleX, danger**, Lock → Lock, Pin → Pin, Copy Last Output → ClipboardCopy, Copy Error → TriangleAlert, Clear → Eraser, Export → Download, Full Screen → Maximize |
| Terminal pane menu | `terminal/tab/menu.rs` | Copy → Copy, Paste → ClipboardPaste, Select All → SquareDashed, Find… → Search, Copy Last Output → ClipboardCopy, Copy Error → TriangleAlert, Open Link → Globe, Copy Link → Link, Open File → File, Clear → Eraser, Reset Terminal → RotateCcw, Rename Pane → Pencil, Split Right/Down → Columns2/Rows2, Zoom Pane → ZoomIn, Full Screen → Maximize, Send Right-Clicks to Pane → check (toggle), Close Pane → X |
| File tree | `files/render.rs` | New File → FilePlus, New Folder → FolderPlus, Reveal in Finder → FolderOpen, Open in Default App → ExternalLink, Open in Terminal → SquareTerminal, Find in Folder… → Search, Cut → Scissors, Copy → Copy, Duplicate → CopyPlus, Paste → ClipboardPaste, Undo → Undo2, Redo → Redo2, Copy Path / Copy Relative Path → Link, Add to .gitignore / .git/info/exclude → EyeOff, View History → GitGraph, Expand All / Collapse All → UnfoldVertical / FoldVertical, Rename → Pencil, **Move to Trash → Trash, danger**, **Delete → Delete, danger** (the destructive group moved last) |
| Config files | `shell_config/render.rs` | Open → File, Reveal in Finder, Open in Default App, Open in Terminal, Duplicate, Copy Path, **Move to Trash, Delete: danger** |
| Git panel: branches | `git/panel/branches.rs` | branches (check on the current), Remote Branches ▸ → Cloud (each → GitBranch), New Branch… → Plus, Merge into ▸ → GitMerge (each → GitMerge) |
| Database panel | `db/panel/add.rs`, `db/panel/rows.rs`, `db/table/export.rs` | Add: PostgreSQL / D1 / SQLite → their logos in the icon slot (`leading`); rows and export had icons |
| SSH host | `ssh/panel/host.rs` | Connect → Plug, New Session → SquareTerminal, Copy SSH Command → Copy (Check once copied), Check Connection → Stethoscope, Edit → SquarePen, Duplicate → CopyPlus, **Delete… → Trash, danger**, Save as Server → Save |
| Pages, Update Skills, footer | `workspace/pages_menu.rs`, `sawit_skills/button.rs` | had icons; the look comes from the component |
| Accounts page | `accounts/page.rs`, `list.rs`, `statusline_tab.rs`, `usage_tab.rs` | Rename… → Pencil, Check Sign-In Again → RefreshCw, **Remove from Sawit… → Trash, danger**; add folder → FolderInput; status line kinds → Plus, themes → Palette, presets → LayoutTemplate; Continue On… accounts → CircleUser |
| Catalog pages | `catalog/toolbar.rs`, `catalog/pager.rs`, `package_managers.rs` | had icons or checks |
| New Project | `new_project_page/render.rs` | Presets → LayoutTemplate, Create Project Only → FolderPlus |
| Permissions | `permissions_page/sections.rs` | Move to … → MoveRight, **Remove Rule… → Trash, danger** |
| Browser toolbar, Annotate | `browser/device.rs`, `browser/annotate/send.rs`, `browser/design/colors/mod.rs` | had icons, checks or agent logos |
| Canvas main menu, context menu | `board/menus/popup.rs`, `board/menus/icons.rs` | every engine command → its icon (`icons::command`, by id, its menu's icon as the fallback), every submenu → its icon (`icons::submenu`); Send to Agent's agents → their logos |
| Canvas toolbar flyouts, design panel, layers | `board/toolbar/*`, `board/design_panel/*`, `board/layers_panel/rows.rs`, `board/context_bar.rs` | pictures of their choices (`option_icons`) or checks |
| Canvas presence | `board/presence/edges.rs`, `roster.rs` | agents → Bot, works → LayoutTemplate |

## Floating bars

| Bar | Where | Buttons → icon |
|---|---|---|
| Route frame | `board/routes/bar.rs` | Live → Radio, Reload → RefreshCw, Full Page → UnfoldVertical (toggle: accent icon, check, selected fill, `aria-pressed`), │ Open in Browser → Globe |
| Screen (B13) | `board/screens/bar.rs` | Preview → Eye, │ Export ▾ → Download (HTML → FileCode, React → Code, PNG → Image), │ Send to Agent ▾ → Bot (primary) |
| Artboard (B13) | `board/screens/frame_bar.rs` | Export ▾ (HTML, React, PNG, SVG → PenTool), │ Send to Agent ▾ (primary) |
| Flow (B13) | `board/screens/frame_bar.rs` | Send Flow to Agent ▾ (primary) |
| Timer and vote pill | `board/facilitation.rs` | Timer 5:00 → Timer; while it runs: the time (tabular figures, danger at 0:00), Pause/Resume → Pause/Play, 1 Min → Plus, Stop → CircleStop; │ Start Vote → Vote, Results → ChartColumn; while voting: Vote on Selection → Vote, End Vote → SquareCheck |
| Context bar | `board/context_bar.rs` | already pictured; now the same 36 px height and 10 px radius as the others |
| Preview | `board/screens/preview.rs` | the title, Size ▾ (one menu of the sizes with their device icons and a check, instead of a scrolling row of buttons), Rotate → RotateCw (toggle), │ Close → X |

Every bar on the canvas is as wide as its buttons plus 4 px from its edge; on a canvas too narrow
for it, its buttons show their icons alone with the label in the tooltip and the accessible name.
