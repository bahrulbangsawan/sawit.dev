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

#### Task 3 notes: live check (2026-10-30, at `ef8c209`, lane 3)

Steps 1–3 passed over the control socket, and so did the checks that the delegates' rulings on
Task 2 carried to this one: the rename field, the divider page, and the panel beside a board's
context bar and toolbar. The check found no defect in the panel, so it has no fix commit. It did see
one change that no op of the check made, and undid it with ops (see "A change that no op of this
check made"). Step 4 needs the user: its checks are under "Hand checks (pending — needs the user)"
below, with the ones those rulings asked for. `cargo fmt --all -- --check` and
`cargo clippy --workspace --all-targets -- -D warnings` are clean at `ef8c209`, and
`cargo test --workspace` passes: 1245 passed, 9 ignored, 0 failed (sawit 242, sawit-board 597,
sawit-core 324 and 23 in its integration tests, sawit-mcp 49 and 10). Machine: Apple M3 (8 cores,
16 GB), macOS 26.6.1, window scale factor 2, debug build. Other lanes were compiling, so the timing
is under build load (R-LOAD): `vm.loadavg` was 4.2 at the build, 3.6 at the first launch and 28 when
the second instance stopped.

**How it ran.** As Plan 1 Task 7's launch recipe has it (R-LAUNCH), from lane 3, in two runs on one
scratch home, each stopped with `/bin/kill` on its own PID:
- Run 1 (22:37–22:50): a copy of `ridgeline-app.figcraft` (SHA-1 `3726c59b…`) as the scratch
  project's `.nyawit/canvas.figcraft`. Steps 1–3, the rename and divider carries, and the extra
  checks.
- Run 2 (22:50–22:52): an empty project, for the board check (P4-C14). The context bar is drawn only
  on boards, so it never shows on ridgeline.
- After `open_page`, the canvas painted one frame before any other op (the P3.T4 ruling). The state
  was read between batches (`get`, `page.list`, the file's SHA-1, and the zoom label that every
  screenshot shows), which is how the change below was seen.
- Each batch of screenshots began with `{"cmd":"focus"}`, the one call that raises the window, and
  every shot waited for the window to be on screen with nothing over it.
- Run 1 sent `{"cmd":"layout","terminal":false}` once, to give the canvas the terminal's room. The
  window kept the size Sawit opens it at (1280 × 801 pt). No accessibility call and no posted event
  reached any app. One read-only `osascript` query of the frontmost app's name (22:40) went against
  the P5.T6 carry: it changed nothing and was not repeated.
- Only the check changed the document, with ops, and each change was undone or reversed: the last
  `get` of run 1 listed the file's 4 layers, and `page.list` its one page.
- The panel is 240 pt wide (x = 240 to 480 pt of the window) between two 1 pt borders (`#363c46`).
  Its background is `#282c33`, and a selected row's or the current page's is `#2e323a`. Its rows are
  28 pt high (56 px at 2×).

**Answers (Steps 1–3, as P4-C15 amends them).**

- Step 1. `open_page canvas` answers `{"tab":0}`. `get` lists the 4 top-level layers: `"349"`
  Components (a section), `"68"` Home — Light, `"239"` Home — Dark and `"178"` Trail — Light. The
  file is a design file, so it opened with the panel shown, and the Design panel too.
  - The screenshot shows the panel: the header "Pages" with a chevron and a "+" button; the page
    "App", marked as current; the filter "Find layers" with its search icon; then the tree, top of
    the stacking order first: Trail — Light, Home — Dark, Home — Light (the frame icon) and
    Components (the section icon), each with a closed arrow and none selected. No eye or lock
    shows. The Design panel reads "Select a layer to see its design."
- Step 2. `select.all` answers `{"selection":[349,68,239,178]}`. The 4 rows are highlighted and none
  opens: their only ancestor is the page, which reveal leaves out, so reveal can't be seen from it
  (P4-C15). The Design panel reads "4 layers", and the zoom stays 15 %.
  - `select.children` then answers 16 ids: `[14,19,28,29,55,69,79,160,240,250,331,179,185,195,202,238]`.
    The 4 frames open, the 16 child rows are highlighted and the 4 parent rows aren't. Down the
    panel the highlighted runs are 280, 168, 168 and 280 px: 5, 3, 3 and 5 rows of 56 px (under
    Trail — Light, Home — Dark, Home — Light and Components). The Design panel reads "16 layers".
    Neither op makes the canvas render: the log gets no line.
- Step 3. `object.toggleVisible` with `params: {}` answers `{"visible":false}`. All 16 rows show an
  eye-off icon at their right edge and their names are muted, and the 4 parents show none. The canvas
  draws the frames without their children, and the Design panel's Appearance shows the eye-off icon
  too. One `edit.undo` makes the 16 layers visible again: every eye-off icon goes and the selection
  returns to the 16, so the panel follows undo.

**The carries from P4.T2's rulings.**

- Rename (a). `select.set {ids: [68]}`, then `object.rename {}` (a rename with no name is the UI
  request `layers.rename`). The "Home — Light" row holds a text field with its name selected, a
  caret after it, and the focus ring. The row keeps its 56 px band (945–1000 px, as before) and the
  rows round it haven't moved. The field's border box is 32 px (16 pt) high, and with its 6 px ring
  it fits whole inside the row.
  - `view.layers` twice (the panel goes and comes back; each toggle makes the canvas render once,
    since its width changes by 240 pt) leaves the tree as it was: the field is gone, the open rows
    stay open, and `get` still reads `Home — Light`. One `edit.undo` then made the 16 hidden layers
    visible, so the field's blur ran no command: a rename step would have taken the undo instead.
    The `view.layers` ops also show P4-C16: a command op's UI request is carried out at once.
- Divider (b). `page.add {name: "-"}` answers `{"id":350,"divider":true}`, and `page.list` lists
  App (current) and `-`, a divider with no children. The screenshot shows a 1 pt line (2 px,
  `#363c46`) in a row of its own under App, 9 pt in from each edge, with the canvas still on App.
  - `page.add {name: "Two"}` answers `{"id":351}` and opens it: `get` answers `[]`, the page list
    marks Two, and the tree says "No layers on this page". `page.delete {id: 351}`, the current page,
    leaves App current, with its 4 layers and its 15 % view. `page.delete {id: 350}` removes the
    divider. `page.delete {id: 1}`, the only page, answers
    `{"ok":false,"error":"a file needs at least one page"}` and the page stays. (A control op
    answers its error in the reply and shows no toast. The panel's own Delete shows one: that is in
    the hand checks.)
- The board with the panel (P4-C14, run 2). `open_page canvas` on an empty project, `tool.sticky`,
  `pointer` down and up at (0, 0) (answers `selection: ["3"]`, the sticky's text, and
  `tool: "move"`), `key Escape` and `select.all` (answers `[2]`). `get` lists the sticky `"2"` at
  (−100, −100), 200 × 200, `#FFE78A`. The board opened with the panel hidden (D11), and `view.layers`
  showed it.
  - The panel lists `Page 1` and one selected row, "Sticky", with the sticky-note icon and no child
    row (its text is hidden). The top bar's layers toggle is marked (`#454a56`; the Design toggle
    stays `#282c33`).
  - The context bar is 220 × 40 pt and centred at x = 880 pt, the canvas's middle
    ((480 + 1280) / 2), and its bottom edge is 12 pt over the sticky's top edge (415 px to 439 px).
    The toolbar is 508 pt wide and centred at 879.75 pt. The window's own middle is 640 pt.
  - Without the panel both are centred at 760 pt ((240 + 1280) / 2), so showing the panel moves
    both by 120 pt, half its width.

**Also checked over the socket.**

- `layers.collapseAll` (P4-C9). With no selection it closes every row. It reset the panel 5 times.
- ⌘\ (P4-C8). `{"op":"key","key":"\\","cmd":true}` answers `handled: true`: the top bar, both panels
  and the toolbar go, and the canvas takes their room. Again, and all come back, the layers panel
  with them. `view.leftSidebar` (P4-C10) hides and shows the layers panel alone. (The shots tell them
  apart by the "App" row's pixel: `#2e323a` with the panel, the canvas's `#1e1e1e` without.)
- Eye and lock. `object.toggleLock` on Home — Light puts a lock icon on its row, and
  `object.toggleVisible` on Home — Dark an eye-off icon and a muted name. The other rows show
  neither. Two `edit.undo` took both away.
- Following the canvas. `select.byName {query: "Landscape"}` finds node 14 and `select.children`
  selects its two children, `[2, 8]`. The tree had 20 rows, with about 20.8 in view. The panel
  opened Landscape (22 rows) and scrolled the list about 4 pt, just far enough to bring the first
  selected row, "Time=Dusk", whole into view at the bottom edge.
- A double-click on a frame's name on the canvas (the Plan 2 final review's carry).
  `{"op":"pointer","kind":"double","x":165,"y":-69}`, at the name of Home — Light at 15 %, answers
  `selection: ["68"]` and starts the rename in that layer's row, its name selected.
- Several ids (P4-C11). `select.set {ids: [68, 239]}`, then `object.rename {}`: only "Home — Dark",
  the topmost of the two in the list, gets the field, and the selection is `[239]` alone.
- Board kinds (P4-C4). `board.template {name: "flowChart"}` and `board.table {rows: 3, columns: 3}`:
  the tree shows the Table with the table icon, connectors and connector labels (rows named "Label")
  with the spline icon, and shapes with the shapes icon, and none of them has child rows.
- A deep tree (Review Focus 5). `node.create` made a rectangle (`"352"`, "deep leaf") and one
  `command.batch` of 200 `object.group` steps wrapped it in 200 groups (`"353"` to `"552"`, the top
  one "Group 200") in 15 ms. `select.set {ids: [352]}` opened all 200 ancestors and scrolled to the
  leaf's row, the last of 205; `select.set {ids: [552]}` scrolled back to the first. A `get` straight
  after each answered in under a millisecond, and the shots were taken a second later. One
  `edit.undo` took all 200 groups away (the batch is one step), and a second the leaf.
- The logs. Run 1 has 86 lines and run 2 has 5, all `canvas render`: no warning and no error. Run
  1's renders, in ms, in the debug build under build load: min 21.7, p50 39.4, p95 121.5, max 488.0
  (the first render). They are no budget: P8.T4 measures that.

**A change that no op of this check made.** In run 1, between the first screenshots (22:38:47) and
the one taken after `select.all` (22:39:26), the view changed on its own: the zoom went from 15 % to
45 %, and the panel's rows Home — Light, its Content, Components and its Trail row were open. The
file's SHA-1 was still the original's and `get` was unchanged, so nothing was edited. By 22:40 the
log held 63 `canvas render` lines, far more than the few ops so far could make (a selection op makes
none), many of 21–28 ms in a row, as a continuous zoom gesture leaves. `{"cmd":"focus"}` had raised
the window at 22:38:46, and the frontmost app was Terminal again at 22:40. The likely cause is a
person at the machine using the window, not yet confirmed (the P3.T4 ruling reads the same sign the
same way).
- Undone with ops: `select.none`, `layers.collapseAll` and `view.zoomFit` (zoom 0.1546, 15 %). The
  screenshot then matched the first.
- Step 2 was run again with the log counted per op: `select.all` and `select.children` each add no
  line and leave the zoom at 15 %, so the ops don't cause the change.

**Seen, not defects.**
- Deep rows run off the panel. Rows indent 12 pt a level, so a name is cut from about depth 11
  ("Gro…"), from about depth 14 only the arrow and icon are left, and from about depth 19 nothing of
  a row is inside 240 pt. The leaf of the 200-deep chain is a highlighted row with nothing in it.
  FigCraft's own panel indents 4 + 16 px a level with no cap, so it runs off the same way, and the
  spec asks only for a virtualized list. A cap on the indent, or a horizontal scroll, would be new
  work: say if you want it.
- Connector labels are rows of their own, named "Label" (P4-C4).
- A control op that the engine refuses answers `{"ok":false,"error"}` and shows no toast.

#### Hand checks (pending — needs the user)

Launch with Plan 1 Task 7's launch recipe, with a copy of `ridgeline-app.figcraft` as
`.nyawit/canvas.figcraft` before the launch (these checks edit the file), and run its `open_page`
line. A design file opens with the layers panel on the left and the Design panel on the right. Click
the canvas once, so that it has the keyboard. In this file "Home — Light" holds the frames "Tab bar",
"Content" and "Status bar"; "Content" holds instances ("Trail row", "Featured") and frames;
"Trail — Light" holds "Home indicator" (a rectangle), "Hero" (an instance) and others; "Components"
is a section of components ("Trail row", "Trail card", "Button", "Chip", "Landscape"). Where a check
already passed over the socket, it says so. Record your answers under each number.

1. **Drag to reorder** (Step 4).
   - Open "Home — Light" with its arrow. Press on "Status bar" and drag a few pixels: a small label
     with its name follows the pointer.
   - Move over the top half of the "Tab bar" row: a 2 px line shows in the gap above it, from the
     row's indent to the panel's right edge. Over its bottom half the line moves below it. Over the
     gap where the layer already is, no line shows: a drop that moves nothing shows no mark.
   - Drop above "Tab bar": "Status bar" is now the first of the three, and the canvas shows the new
     stacking. One ⌘Z puts the order back, and ⇧⌘Z does it again.
   - ⌘-click two rows and drag one of them: the label reads "2 layers", both move, and one ⌘Z puts
     both back.
2. **Drag into a frame** (Step 4).
   - Drag "Status bar" onto the middle half of the "Content" row. A frame (not a line) goes round
     the row. Drop: "Status bar" is now one of Content's children, and one ⌘Z puts it back.
   - Over the middle of a row that takes no layers (an instance such as "Featured", or the rectangle
     "Home indicator"), no frame shows, only the lines above and below it.
3. **Drops that must be refused** (Review Focus 1).
   - Drag "Home — Light" over its own child "Content" and over the rows under it: no mark shows on
     the dragged row or under it, and a release there changes nothing: the order, the selection and
     the undo history stay (⌘Z undoes the step before the drag).
   - Drag the section "Components" over the middle of "Home — Dark": no frame shows, since sections
     go only on the page or into a section.
   - Open the instance "Hero" in Trail — Light, drag one of its children onto the middle of
     "Home — Dark": the frame shows (the panel leaves what lies inside an instance to the engine).
     Release: a toast says why the engine refuses, and the children, the selection and the undo
     history are unchanged.
4. **Rename** (Step 4, and Review Focus 2). The field's start passed over the socket; typing and
   keys need you.
   - Double-click "Home — Dark" in the panel: a field replaces the name, the whole name selected.
     Type `Dark home` and press Enter: the row, the label on the canvas and the Design panel's
     header read "Dark home". One ⌘Z puts "Home — Dark" back.
   - Double-click it again and press Escape: the old name stays, and ⌘Z undoes the step before.
     Double-click, clear the text and press Enter: the old name stays and no undo step is added. The
     same for a name of only spaces.
   - Double-click, type a few letters and click another row or the canvas: the typed name is kept
     (the field losing the keyboard keeps it).
   - Click a layer on the canvas (it is selected and the canvas has the keyboard) and press F2, or
     ⌘R: the field opens in that layer's row. Double-click a frame's name on the canvas, the label
     above the frame: the same. (Both started over the socket, with ops and not the real keys or
     mouse.) With a long list, scroll that layer's row out of view first: the list scrolls back to
     it (Task 2's review found no test of this on a list that scrolls).
   - Press ⌘\ to hide the chrome, then F2 on a selected layer: nothing shows. Press ⌘\ again.
     Task 2's review expects the field to appear then, its name selected and holding the keyboard,
     so that a letter typed replaces the name (a deferred minor). Say whether that matters.
5. **The filter** (Step 4, and Review Focus 3).
   - Click "Find layers" and type `card`: the tree lists the layers whose names hold "card" under
     the layers that hold them, open (at least Components, then Trail card), and hides the rest. The
     arrows do nothing while it filters.
   - With the field still holding the keyboard, type `v`, `r` and `t`, the keys of the Move,
     Rectangle and Text tools: they appear in the field and the tool doesn't change. Then click the
     canvas and press `r`: the Rectangle tool comes on, so the same key acts there.
   - Type `zzzz`: the tree says "No matching layers" and shows no rows, never the old tree.
   - Clear the field (the ✕ in it, or delete the text): the tree is as it was before the filter. The
     rows you had opened are open, the ones you had closed are closed, and nothing that the filter
     showed stays open.
6. **Hover highlight** (Step 4, and the P2.T3 ruling).
   - With the Move tool (V), hover the row of a layer that isn't selected: the canvas outlines that
     layer 1.5 px in blue (purple in a component), and the row takes a light tint. A selected layer
     gets no extra outline.
   - Move the pointer off the rows, onto the canvas: the outline goes (P4-C12).
   - With another tool (say Rectangle) no outline shows, because the engine sets the hover only for
     Move and Scale. This is known.
   - Rest the pointer on a row and press ⌥1: the panel hides. Task 2's review expects the layer's
     outline on the canvas to stay until the pointer moves, and the row to be drawn hovered when
     the panel comes back (a deferred minor). Say whether that matters.
7. **Pages** (Step 4, and Review Focus 4).
   - Click "+" in the Pages header: "Page 2" is added and opens, and the tree says "No layers on this
     page". Click "App": it opens again, as you left it. Click the "Pages" title: the list folds,
     and opens again with another click.
   - Double-click a page's name: a field with the name selected. Enter keeps what you typed, Escape
     drops it, and a blank name keeps the old one.
   - Right-click a page: Rename page, Duplicate page and Delete page. Duplicate makes "Copy of …"
     right after it, with the same layers, and opens it. Delete the open page: the first page opens.
     Delete the only page: a toast says a file needs at least one page, and the page stays.
   - Rename an empty page to `-`: it becomes a divider line, which a click doesn't open. (The line
     passed over the socket.)
8. **Selecting** (Plan 4's goal).
   - Click a row: that layer is selected on the canvas and in the Design panel. Press `r`: the
     Rectangle tool comes on, since the canvas has the keyboard and not the panel.
   - ⇧-click another row: the rows between the two are selected. ⌘-click a row: it joins the
     selection, or leaves it when it was in.
9. **Eye and lock** (the spec's "eye and lock appear on hover or when off"). The icons and their
   effect passed over the socket; the mouse needs you.
   - Hover a row: a padlock and an eye appear at its right end, with the tooltips "Lock" and "Hide".
     Click the eye: the layer hides, its name goes grey, and the eye-off stays when the pointer
     leaves. Click it again: the layer shows.
   - Click the padlock: the layer locks, a click on it on the canvas selects nothing, and the
     padlock stays after the pointer leaves. Click it again: unlocked.
10. **Scrolling** (P4-C14, and the spec's virtualized list).
    - Press ⌘A and then Enter (select all, then select children): the 4 frames open, with 20 rows.
      With the terminal showing, the list is longer than the panel. With it hidden, about 20 rows
      fit, so open a few more rows with their arrows until the list scrolls.
    - A two-finger scroll over the list scrolls the list and never pans the canvas. A two-finger
      scroll over the canvas pans the canvas and the list stays. A pinch over the list doesn't zoom
      the canvas. The list scrolls smoothly.
11. **The keys of the panel** (P4-C8 to P4-C10: the socket ran the commands, not the keys).
    - ⌥1 shows and hides the layers panel (on a board too, where it starts hidden). ⇧⌘\ does the
      same. ⌥L closes every row but the ones that lead to the selection. ⌘\ hides all the chrome,
      the panel with it, and brings it back.
