# Native canvas 4: Layers panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A left Layers panel with two parts:
- **Pages:** list, add, rename, duplicate, delete, select.
- **Layer tree:**
  - kind icons, eye and lock toggles
  - click, ⇧-click and ⌘-click selection
  - inline rename, expand and collapse
  - drag to reorder or reparent
  - a name filter, and hover highlighting on the canvas

**Architecture:** `sawit-board/src/layers.rs` turns the document into flat rows and works out drop
targets, as pure functions. The app renders the rows in a virtualized `uniform_list` and maps
interactions to engine commands: `select.set`, `object.rename`, `object.toggleVisible`,
`object.toggleLock`, `node.reparent` and `page.*`.

**Tech Stack:** As Plan 1. gpui-kit `Input` for rename and filter; GPUI `uniform_list`.

**Spec:** `docs/superpowers/specs/2026-10-29-native-canvas-design.md` ("Layers panel", D11)

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

1. Dragging a frame into its own child must be refused and change nothing. Test: `drop_into_own_subtree_is_refused` (Task 1).
2. A rename to an empty or whitespace name must keep the old name. Test: `blank_rename_keeps_name` (Task 2).
3. A filter with no matches must show an empty state, not a stale tree. Clearing the filter must restore the expand state. Test: `filter_without_matches_is_empty` (Task 1) and `filter_restores_expansion` (Task 1).
4. Deleting the current page, or the only page, must leave a valid page selected. FigCraft refuses to delete the last one, and the refusal is a toast. Test: `deleting_current_page_selects_another` (Task 2).
5. A deep tree (200 nested groups) must build rows quickly and scroll. Test: `deep_tree_rows_are_linear` (Task 1), which checks 200 rows build in under 20 ms in debug.

---

### Task 1: Rows, pages and drop targets

**Files:**
- Create: `crates/board/src/layers.rs`
- Modify: `crates/board/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum LayerIcon { Frame, Section, Group, Component, Instance, Text, Rectangle, Ellipse, Vector, Line, Image, Sticky, Shape, Connector, Ink, Stamp, Table, Other }
  pub struct LayerRow { pub id: NodeId, pub depth: usize, pub name: String, pub icon: LayerIcon, pub visible: bool, pub locked: bool, pub has_children: bool, pub expanded: bool, pub selected: bool }
  pub struct PageRow { pub id: NodeId, pub name: String, pub current: bool, pub divider: bool }
  pub enum DropPos { Above, Below, Into }
  pub struct Reparent { pub ids: Vec<NodeId>, pub parent: NodeId, pub index: usize }   // index bottom-first, as node.reparent wants
  pub fn rows(doc: &Document, page: NodeId, expanded: &HashSet<NodeId>, selection: &[NodeId], filter: &str) -> Vec<LayerRow>;
  pub fn pages(doc: &Document, current: NodeId) -> Vec<PageRow>;
  pub fn drop_target(doc: &Document, rows: &[LayerRow], dragged: &[NodeId], over: usize, pos: DropPos) -> Option<Reparent>;
  pub fn reveal(doc: &Document, ids: &[NodeId]) -> HashSet<NodeId>; // ancestors to expand so a canvas selection is visible
  ```
- **Row order:** top of the z order first, i.e. children reversed, as Figma shows them.
- **Board items:** they use their board icon. Their internal children (the sticky's text, the shape's
  outline) are hidden, so `has_children` is false.
- **Filter:**
  - Matching is case-insensitive on the name.
  - A match keeps its ancestors and shows them expanded.
  - The `expanded` set passed in is never changed.
- **`drop_target`:**
  - `Into` is allowed only for frame-like and group parents.
  - `Above` and `Below` resolve to the row's parent, with the bottom-first index worked out.
  - A target inside any dragged node's subtree is refused (`None`).
- **Divider pages:** names starting with `-`.

- [ ] **Step 1: Write the failing tests** (design doc: frame F with children A, B and a nested group G{C}):
  - `rows_list_top_first_with_depth`: all expanded → `[F(0), B(1), A(1), G(1), C(2)]`, in the actual z order of the fixture.
  - `collapsed_hides_children`: nothing expanded → `[F]`, with `has_children` true.
  - `filter_keeps_ancestors`: filter `"c"` → `[F, G, C]` with F and G expanded.
  - `filter_without_matches_is_empty`.
  - `filter_restores_expansion`: the `expanded` set is untouched after a filtered call.
  - `selected_rows_are_marked`.
  - `sticky_has_no_children_rows`: a board sticky → one row with `LayerIcon::Sticky`.
  - `drop_between_siblings_gives_bottom_first_index`: drag A above B → `Reparent { parent: F, index }`. Applying it with `node.reparent` puts A just above B.
  - `drop_into_frame`: drag C `Into` F → parent F.
  - `drop_into_own_subtree_is_refused`: drag F `Into` G → `None`.
  - `pages_list_and_dividers`: add pages `"Two"` and `"-"` → 3 rows, with one divider and `current` on the first.
  - `reveal_expands_ancestors`: `reveal([C]) == {F, G}`.
  - `deep_tree_rows_are_linear`: 200 nested groups, all expanded → 201 rows in under 20 ms.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board layers::`
Expected: FAIL.

- [ ] **Step 3: Implement `layers.rs`.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board layers::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/layers.rs crates/board/src/lib.rs
git commit -m "feat(canvas): layer rows, pages and drop targets for the layers panel"
```

### Task 2: Layers panel view

**Files:**
- Create: `crates/app/src/board/layers_panel.rs`
- Modify: `crates/app/src/board/mod.rs`:
  - state: `show_layers` (default: false on boards, true on design files), `expanded: HashSet<NodeId>`,
    `layer_filter`, `renaming: Option<NodeId>`, `drag: Option<LayerDrag>`
  - a top-bar toggle
  - the `ui_requests` kind `panel.layers` (from `view.layers`) toggles the panel
  - `UiRequest "layers.rename"` starts a rename of the selection

**Interfaces:**
- Consumes: `layers::*`.
- Produces:
  ```rust
  impl BoardView {
      pub fn select_rows(&mut self, row: usize, shift: bool, cmd: bool, cx: &mut Context<Self>); // select.set with a range or toggle
      pub fn rename_layer(&mut self, id: NodeId, name: &str, cx: &mut Context<Self>);            // object.rename {ids:[id], name}; blank → no-op
      pub fn toggle_visible(&mut self, id: NodeId, cx: &mut Context<Self>);                      // object.toggleVisible {ids:[id]}
      pub fn toggle_locked(&mut self, id: NodeId, cx: &mut Context<Self>);                       // object.toggleLock {ids:[id]}
      pub fn drop_rows(&mut self, over: usize, pos: DropPos, cx: &mut Context<Self>);            // node.reparent from drop_target
      pub fn page_action(&mut self, action: PageAction, cx: &mut Context<Self>);                 // add | rename | duplicate | delete | select
  }
  pub enum PageAction { Add, Rename(NodeId, String), Duplicate(NodeId), Delete(NodeId), Select(NodeId) }
  ```
- **Panel:** 240 px wide. The pages list sits on top and is collapsible; the filter input is below it;
  the tree comes last.
- **Rows** are 28 px, with the indent = `depth · 12 px`.
- **Eye and lock** appear on hover, or always when the layer is hidden or locked.
- **Double-click on a name** opens an `Input` in the row. Enter commits, Escape cancels.
- **Hover** sets `ds.hover` and calls `cx.notify()`.
- **Reveal:** when the canvas selection changes, `expanded ∪= reveal(selection)` and the list scrolls to
  the first selected row.

- [ ] **Step 1: Write the failing tests** (`#[gpui_kit::test]` on a `BoardView` with a design fixture):
  - `click_selects_and_shift_selects_range`: `select_rows(0, ..)`, then `select_rows(2, shift)` → 3 selected.
  - `cmd_click_toggles`.
  - `blank_rename_keeps_name`: `rename_layer(id, "  ")` → the name is unchanged and there is no undo entry.
  - `eye_and_lock_toggle`.
  - `deleting_current_page_selects_another`: add a page, select it, delete it → the current page is the first one. Deleting the only page → a toast is set and the page is still there.
  - `drop_reorders`: `drop_rows` moves A above B, and one `edit.undo` restores the order.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit board::layers`
Expected: FAIL.

- [ ] **Step 3: Implement** `layers_panel.rs` and the state in `mod.rs`.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/board
git commit -m "feat(canvas): layers panel with pages, selection, rename, visibility, lock and drag"
```

### Task 3: Live verification

- [ ] **Step 1:** Launch with the flag, opening `ridgeline-app.figcraft` as the canvas (Plan 3 Task 4).
- [ ] **Step 2: Check the panel's initial state.**
  - Expected: the screenshot shows the Layers panel with its pages and the frame tree.
  - Send `{"op":"command","command":"select.all"}`.
  - Expected: the selected rows are highlighted and their ancestors expanded.
- [ ] **Step 3: Check that hiding a layer reaches the panel.**
  - Send `{"op":"command","command":"object.toggleVisible","params":{}}`.
  - Expected: the screenshot shows eye-off icons on the selected rows.
- [ ] **Step 4: Ask the user to check by hand:**
  - drag reorder
  - drag into a frame
  - rename
  - the filter
  - hover highlight
  - page add, rename and delete
  - Record their answers.
- [ ] **Step 5: Stop the instance** with `/bin/kill`, then run the full gates:
Run: `cargo test --workspace`
Expected: all pass.
