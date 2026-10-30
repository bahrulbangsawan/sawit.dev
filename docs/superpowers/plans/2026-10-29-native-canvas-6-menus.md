# Native canvas 6: Menus, context menu, Actions and shortcuts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** This plan adds:
- the ≡ main menu, built from FigCraft's menu catalog under Sawit's menu policy
- the right-click context menu
- the Actions palette (⌘K, ⌘/)
- the keyboard-shortcuts sheet (⌃⇧?)
- implementations of the UI-handled commands: import, place image, save, save as, export, export SVG,
  export frames PDF, and copy as text/CSS/SVG/PNG

**Architecture:** `sawit-board` builds the menu trees, the action list and search, and the shortcut sheet
as plain data (`menu.rs`, `actions.rs`, `shortcuts.rs`). The app renders them with gpui-kit `PopupMenu`
(nested submenus, `checked`, `disabled`) and a palette list. It implements the UI-handled ids in
`board/commands.rs`, using GPUI dialogs (`cx.prompt_for_paths`, `cx.prompt_for_new_path`) and the
clipboard.

**Tech Stack:** As Plan 1. gpui-kit `PopupMenu`/`PopupMenuItem` (`.submenu`, `.checked`, `.disabled`, `.on_click`), `ContextMenuExt`, `Input`, `Kbd`.

**Spec:** `docs/superpowers/specs/2026-10-29-native-canvas-design.md` ("Menu policy", "Context menu", D8)

## Global Constraints

- Toolchain: Rust 1.99.1 (`rust-toolchain.toml`), edition 2024, workspace member crates only.
- FigCraft deps are pinned in root `[workspace.dependencies]` to rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`.
- `crates/board` never depends on GPUI. All GPUI code lives in `crates/app/src/board/`.
- User-facing name stays "Canvas". Chrome uses `cx.theme()` and `Lucide` icons.
- The native canvas is reachable only when `Workspace::native_canvas` is true. Plan 8 removes this.
- Engine errors become a toast. No `unwrap`/`expect` outside tests.
- Gates before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the touched crates' tests.
- Commits:
  - Commit on `main` with explicit `git add <paths>`.
  - Conventional Commits with lowercase subjects. Every message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - **Never `git push`.**

## Review Focus

1. No hidden command may appear in any menu, the palette or the shortcut sheet. Test: `no_hidden_ids_anywhere` (Task 1).
2. A menu command the engine refuses (`Disabled`) must show its reason as a toast, not fail silently. Test: `disabled_command_toasts_reason` (Task 3).
3. Export with nothing selected must export the whole page. Export of a page with no frames to PDF must explain why. Tests: `export_without_selection_exports_page`, `frames_pdf_without_frames_explains` (Task 3).
4. A cancelled Save As or Import dialog must change nothing and show nothing. Test: `cancelled_dialog_is_a_no_op` (Task 3).
5. Copy as PNG of a huge selection must be capped: the longest side is at most 4096 px, and the scale is reduced to fit. Test: `copy_png_is_capped` (Task 3).

---

### Task 1: Menu, context menu, actions and shortcuts models

**Files:**
- Create: `crates/board/src/menu.rs`
- Create: `crates/board/src/actions.rs`
- Create: `crates/board/src/shortcuts.rs`
- Modify: `crates/board/src/lib.rs`

**Interfaces:**
- Consumes: `policy::{is_hidden, is_ui, SAWIT_WINS, SAWIT_WINS_UNLESS_EDITING}` (Plan 2), `figcraft_engine::{catalog, keymap, cmd::view::checked, cmd::find_command}`.
- Produces:
  ```rust
  pub enum MenuNode {
      Item { id: &'static str, label: String, shortcut: Option<String>, checked: Option<bool>, enabled: bool },
      Submenu { label: String, children: Vec<MenuNode> },
      Separator,
  }
  pub const TOP_MENUS: &[&str] = &["File", "Edit", "View", "Object", "Text", "Arrange", "Vector", "Preferences", "Help"];
  pub fn main_menu(s: &Session) -> Vec<MenuNode>;     // catalog::MENU order; "A/B" paths → nested Submenus; Help keeps only help.shortcuts
  pub fn context_menu(s: &Session) -> Vec<MenuNode>;  // spec list; Show/Hide and Lock/Unlock labels follow the selection
  pub fn shortcut_label(id: &str) -> Option<String>;  // keymap::primary(id, Mac) → keymap::display; None when the keystroke is in SAWIT_WINS
  pub struct ActionItem { pub id: &'static str, pub label: String, pub path: String, pub shortcut: Option<String> } // path "Object › Group"
  pub fn all_actions(s: &Session) -> Vec<ActionItem>;            // enabled main-menu items, flattened
  pub fn search(items: &[ActionItem], query: &str) -> Vec<ActionItem>;
  pub struct ShortcutGroup { pub name: String, pub rows: Vec<(String, String)> } // (label, display)
  pub fn sheet() -> Vec<ShortcutGroup>;                          // keymap::KEYS by Group; hidden ids and SAWIT_WINS keys dropped
  ```
- Menu building:
  - Separators come from `catalog::SEP`.
  - Separators are collapsed: never two in a row, never one first or last.
  - An empty submenu is dropped.
- `search` ranking:
  1. The label starts with the query.
  2. A word in the label starts with the query.
  3. The query is a subsequence of the label.
  - Matching is case-insensitive. Ties go to the shorter label. An empty query returns everything.
- `context_menu` with an empty selection gives: Paste here, Select all, Zoom to fit, Show/Hide UI.
  These map to `edit.paste`, `select.all`, `view.zoomFit` and `view.toggleUi`.

- [ ] **Step 1: Write the failing tests:**
  - `top_menus_follow_policy`: the labels equal `TOP_MENUS` exactly, in that order.
  - `no_hidden_ids_anywhere`: walk `main_menu`, `context_menu` (both selection states), `all_actions` and `sheet` → `!is_hidden(id)` for every id. `sheet` contains no ⌘W row.
  - `copy_as_is_a_submenu_of_edit`: Edit contains `Submenu { label: "Copy as", children: 4 items }`.
  - `checked_follows_engine`: `view.rulers` is unchecked; after `execute("view.rulers")` it is `checked == Some(true)`.
  - `separators_are_collapsed`: no menu starts or ends with a `Separator`, and none has two in a row.
  - `shortcut_labels_use_mac_glyphs`: `shortcut_label("edit.duplicate") == Some("⌘D")`; `shortcut_label("file.close") == None` (Sawit owns ⌘W).
  - `context_menu_on_selection`: one sticky selected → has `edit.delete`, `arrange.front`, `object.toggleVisible` labelled "Hide", and the Copy as submenu.
  - `context_menu_on_empty_canvas`: exactly 4 items.
  - `search_ranks_prefix_first`: `search(all, "dup")[0].id == "edit.duplicate"`; `search(all, "")` returns everything; `search(all, "zzz")` is empty.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board menu:: actions:: shortcuts::`
Expected: FAIL.

- [ ] **Step 3: Implement** the three modules.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/menu.rs crates/board/src/actions.rs crates/board/src/shortcuts.rs crates/board/src/lib.rs
git commit -m "feat(canvas): menu, context menu, actions and shortcut models from figcraft's catalog"
```

### Task 2: Render the main menu, context menu, palette and sheet

**Files:**
- Create: `crates/app/src/board/menus.rs`
- Modify: `crates/app/src/board/top_bar.rs` (≡ button first)
- Modify: `crates/app/src/board/events.rs` (right click: select the target as `ui-egui/src/canvas/input.rs:269` does, then open the context menu at the pointer)
- Modify: `crates/app/src/board/mod.rs`:
  - `ui_requests` kind `palette.actions` → `palette_open = true`
  - kind `panel.shortcuts` → `sheet_open = true`

**Interfaces:**
- Consumes: `menu::*`, `actions::*`, `shortcuts::*`.
- Produces:
  ```rust
  impl BoardView {
      pub fn run_menu(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>); // is_ui → commands::run_ui (Task 3), else execute; then drain ui_requests
  }
  fn to_popup(nodes: &[MenuNode], view: WeakEntity<BoardView>, window: &mut Window, cx: &mut App) -> Entity<PopupMenu>; // Items → PopupMenuItem::new(label).on_click(..).checked(..).disabled(!enabled); Submenu → PopupMenuItem::submenu
  ```
- Menu items show their shortcut text right-aligned, using the item's label element builder.
- **Palette:**
  - A centred overlay 480 px wide with an `Input` and up to 12 results (label, path, `Kbd` shortcut).
  - Up and Down move the highlight; Enter runs `run_menu`; Escape closes.
  - It opens with ⌘K and ⌘/; `app.actions` reaches the view as `palette.actions`.
- **Shortcuts sheet:** a modal with columns per `ShortcutGroup`. Escape or a click outside closes it.

- [ ] **Step 1: Write the failing tests** (`#[gpui_kit::test]`):
  - `menu_runs_engine_command`: `run_menu("edit.duplicate")` with a sticky selected → 2 stickies.
  - `cmd_k_opens_palette`: `on_key` ⌘K → `palette_open`.
  - `palette_enter_runs_first_result`: palette query `"dup"`, Enter → the sticky is duplicated and the palette is closed.
  - `right_click_selects_target`: right click on an unselected sticky → selection == [sticky] and `context_menu_at.is_some()`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit board::menus`
Expected: FAIL.

- [ ] **Step 3: Implement** `menus.rs` and the glue.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/board
git commit -m "feat(canvas): main menu, context menu, actions palette and shortcuts sheet"
```

### Task 3: UI-handled commands

**Files:**
- Create: `crates/app/src/board/commands.rs`
- Modify: `crates/board/src/ops.rs` (`export_page(board, format, scale) -> Result<Vec<u8>, BoardError>`, shared by Plan 7's `export_svg` and `export_png` ops)

**Interfaces:**
- Consumes: `design::export::export_bytes` (Plan 5), `clipboard::after_copy` (Plan 2), `store::Writer`.
- Produces:
  ```rust
  pub(super) fn run_ui(view: &mut BoardView, id: &'static str, window: &mut Window, cx: &mut Context<BoardView>);
  // sawit-board
  pub fn export_page(board: &mut BoardDoc, format: &str, scale: f64) -> Result<Vec<u8>, BoardError>; // ids = the page's top-level layers
  pub fn copy_png_scale(bounds: (f64, f64), wanted: f64) -> f64; // longest side ≤ 4096 px
  ```
- What each UI id does:
  | Id | Behaviour |
  |---|---|
  | `file.import`, `file.placeImage` | `cx.prompt_for_paths` (files, multiple) → per file: `file.import` / `file.placeImage` with `{data: base64, name}`, placed at the viewport centre |
  | `file.save` | `flush()`, then the toast "Saved" |
  | `file.saveAs` | `cx.prompt_for_new_path(dir, "<name>.figcraft")` → write `to_bytes` |
  | `file.export` | selection (or the whole page) using `export_rows` per layer → `.nyawit/exports/` → `open -R` the first file |
  | `file.exportSvg` | the same, forcing SVG |
  | `file.exportFramesPdf` | `execute("file.exportFramesPdf", {})` → base64 → `.pdf`. A `Disabled` error becomes a toast with the reason. |
  | `edit.copyAsText` | `cx.write_to_clipboard(ClipboardItem::new_string(..))`, text = the plain text of the selected layers joined by newlines (`figcraft_engine::inspect` text props) |
  | `edit.copyAsCss` | `figcraft_engine::codegen::code_for(doc, first, CodeLang::Css)` to the clipboard |
  | `edit.copyAsSvg` | `file.export {ids, format:"svg"}` → decoded text to the clipboard |
  | `edit.copyAsPng` | `file.export {format:"png", scale: copy_png_scale(..)}` as a PNG `ClipboardItem` |
- A cancelled dialog returns without a toast.
- Every engine `Err` becomes a toast with the error text.

- [ ] **Step 1: Write the failing tests:**
  - `sawit-board`:
    - `export_without_selection_exports_page`: `export_page(.., "svg", 1.0)` → valid SVG text with 2 shapes.
    - `copy_png_is_capped`: `copy_png_scale((10000.0, 500.0), 2.0) == 4096.0 / 10000.0`; `copy_png_scale((100.0, 50.0), 2.0) == 2.0`.
  - App (`#[gpui_kit::test]`):
    - `disabled_command_toasts_reason`: `run_menu("file.exportFramesPdf")` on an empty board → the toast contains "no visible frames".
    - `frames_pdf_without_frames_explains`: same assertion through `run_ui`.
    - `cancelled_dialog_is_a_no_op`: drive `run_ui("file.saveAs")` with the test platform's path prompt answering `None` → no file written and `toast.is_none()`.
    - `copy_as_css_writes_clipboard`: select a rectangle, `run_ui("edit.copyAsCss")` → `cx.read_from_clipboard()` contains `"width"`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board ops:: && cargo test -p sawit board::commands`
Expected: FAIL.

- [ ] **Step 3: Implement** `commands.rs`, `export_page` and `copy_png_scale`.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board && cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/ops.rs crates/app/src/board
git commit -m "feat(canvas): import, save as, export and copy as from the canvas menus"
```

### Task 4: Live verification

- [ ] **Step 1:** Launch with the flag and a board containing two stickies.
  - Run `{"op":"command","command":"app.actions"}`.
  - Expected: the screenshot shows the palette.
  - Run `{"op":"key","key":"Escape"}`.
- [ ] **Step 2:** Run `{"op":"command","command":"help.shortcuts"}`.
  - Expected: the screenshot shows the sheet without ⌘W/⌘J rows.
- [ ] **Step 3: Ask the user to check by hand:**
  - the ≡ menu and submenus
  - right-click menus
  - Import an image
  - Export
  - Copy as PNG pasted into Preview
  - Record their answers.
- [ ] **Step 4: Stop the instance** with `/bin/kill`, then run the full gates:
Run: `cargo test --workspace`
Expected: all pass.

#### Task 4 notes: live check (2026-10-31, at `df633b9`, lane 4)

Steps 1 and 2 and the carried socket checks passed. Step 3 needs the user: see "Hand checks" below.
Debug build, launched per R-LAUNCH with a scratch `SAWIT_HOME` and `SAWIT_NATIVE_CANVAS=1`; stopped
with `/bin/kill`. `vm.loadavg` at launch was 4.12 4.43 3.13 (other lanes building). No defect found,
so no code change. Everything was driven over the control socket; no AX, osascript or posted events.

- Board: two stickies (`"2"` at -300,-100 and `"4"` at 100,-100, 200 x 200), made with `tool.sticky`
  and a pointer down/up (the pointer op's field is `kind`, not `type`), Escape after each.
- Step 1, `app.actions`: the screenshot shows the palette over the canvas with 12 rows in menu order
  (Import from file…, Place image…, Save, Save as…, Export…, Export frames to PDF…, Export selection
  as SVG…, Color profile…, Revert to saved, Undo, Redo, Copy as text). Each row that has keys ends in
  a right-aligned keycap (⇧⌘I, ⇧⌘K, ⌘S, ⇧⌘S, ⇧⌘E, ⌘Z, ⇧⌘Z); the rows without keys show none. No
  label is printed twice. `{"op":"key","key":"Escape"}` answers `handled: true` and the palette closes.
- Step 2, `help.shortcuts`: the sheet shows Essential, Tools, View, Zoom, Text and Shape groups in
  three columns. No ⌘W row and no ⌘J row in the visible part; the sheet scrolls, and the part below
  the fold (Object, Vector) was not seen: the hand check lists it. Escape closes the sheet.
- Carry (P6.T2 ruling): with both stickies selected, `view.contextMenu` opens the selection's context
  menu. It is anchored 8 pt under the selection's bottom-left corner, but the shot shows it higher.
  The selection's menu has 25 rows and lines (19 items and 6 lines), more than `SCROLLS_PAST` (20),
  so it scrolls, and gpui-component caps its height at half the window (400 pt in this 801 pt
  window, 450 pt at most). To stay 8 pt inside the window it moved up, over the left sticky, and its
  visible part ends mid-row at Flip horizontal. The rest, Flip vertical to Select all, scrolls into
  view (HC 2). This is by design, not a defect. Cut ⌘X, Copy ⌘C, Paste here ⌘V, Duplicate ⌘D,
  Delete ⌫, Bring to front ⌥⌘], ... show their keys in one right-aligned column, labels at the left.
  Escape closes it. The same `menu_item` builder draws the ≡ menu, so this stands for its key column.
- Carry (P6.T3 ruling): with nothing selected, `file.exportFramesPdf` answers ok with `pages: 2`
  (the stickies are FigCraft frames; kept on purpose under that ruling).
- Follow-up (P6-C18, P6.T1 ruling, P2.T4 ruling), not failures: input dialogs for `text.link`
  (also its keys ⌘K and ⇧⌘U while text is edited), `vector.offset`, `object.moveToPage`,
  `object.batchRename`, `file.colorProfile`, `pref.nudge`; `file.revert` is always refused on a canvas.

##### Hand checks (pending — needs the user)

Run `SAWIT_NATIVE_CANVAS=1 target/debug/sawit` on a project, open the Canvas page, click the canvas
once so it has the keyboard, and put two stickies on it. Record each answer under its number.

1. ≡ menu and each submenu: the keys sit flush right in one column and stay readable on the
   highlighted row. Answer: ______
2. Context menus: a two-finger tap (or the mouse's right button) and a ⌃-click each open the menu at
   the pointer, on an unselected sticky (it becomes the selection) and on empty canvas with something
   selected (the selection clears and the four empty-canvas items show). A ⌃-click does nothing else
   (no drag, marquee or tool stroke). With a sticky selected, the menu stays inside the window, also
   when opened near the window's bottom edge, and a two-finger swipe scrolls it to its last item,
   Select all. Answer: ______
3. Shortcut sheet (⌃⇧? or ≡ ▸ Help): its groups scroll with a two-finger swipe; scroll to the end and
   confirm no ⌘W row and no ⌘J row. Answer: ______
4. Import from file… (⇧⌘I): the open panel's button reads Import, ⌘-click picks two images, both come
   in at the middle of the view, selected. Place image… (⇧⌘K): its button reads Place. Cancel in either
   changes nothing and shows no toast. Answer: ______
5. Save as… (⇧⌘S) on a project's canvas: the save panel opens in the project folder with
   canvas.figcraft in its name field; Save toasts "Saved a copy as canvas.figcraft". Answer: ______
6. Export… (⇧⌘E) with a sticky selected, then with nothing selected: each time a Finder window opens
   on .nyawit/exports/ with the new file selected. Export selection as SVG… and Export frames to
   PDF… do the same. Answer: ______
7. Export frames to PDF… on a canvas with 3 stickies, one inside a section, and a connector between
   the other two, nothing selected: Preview shows 3 pages, one sticky each, ordered by top edge then
   left edge, no page for the section or the connector. Then say whether boards keep this rule or get
   their own (the whole board on one page, or a page per section). Neither answer fails the task.
   Answer: ______
8. Copy as ▸ Copy as PNG (⇧⌘C) with two stickies selected, then Preview ▸ File ▸ New from Clipboard:
   one picture of both, in the canvas's colours. Answer: ______
9. Copy as text, CSS and SVG, each pasted into TextEdit in plain-text mode: the stickies' text one
   line each, the first sticky's CSS with its width and height, and an `<svg …>` document.
   Answer: ______
10. The canvas has the keyboard again (V and H switch tools) after each dialog above closes. It also
    has it after the ≡ menu, the palette, the sheet and the context menu close: on Escape in each,
    and on a choice in the ≡ menu, the palette and the context menu that opens no dialog or sheet.
    Tests check this for the palette, the sheet and the context menu, but not for the ≡ menu.
    Answer: ______

#### Final review fixes (FINAL.P6, 2026-10-31)

- C1: typing in a sticky or a text could be lost. Select none (the empty-canvas right-click of
  ruling P6-C10), a tool or page switch, or deleting the text being typed dropped the edit without
  ending it. That left FigCraft's "Edit text" gesture open: no later command made an undo step, and
  the next click on a layer cancelled the gesture, which put the document back to before the
  typing. Now one BoardView helper, `end_text_edit` (ruling R-TEXTEDIT), ends the edit first, as a
  press outside the text does. It runs at the top of `on_right_down`, before its selection rule, so
  P6-C10 now reads "end the text edit, then clear the selection". It also runs in `run_menu` and
  `choose` before any command but the Text menu's (`text.*`) and copy and paste
  (`sawit_board::policy::keeps_text_edit`). The keys needed no change: while a text is typed in,
  only its own shortcuts run, and ⌘Z ends the edit first. FINAL.P7 routes the layers panel's own
  actions and the command op through the same helper. Tests: four GPUI tests in `board::menus`,
  each failing without the fix, and four sawit-board tests.
- T4-1 and T4-3: HC 10 now also covers the ≡ menu, the palette, the sheet and the context menu
  giving the keyboard back. HC 2 covers the selection's context menu staying in the window and
  scrolling. The context-menu note above now says where that menu sits.
