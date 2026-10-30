# Native canvas 3: Toolbar, top bar and context bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The native canvas gets three pieces of chrome:
- a floating toolbar at the bottom centre, with board and design tool sets and flyouts
- a top bar with the document name, a zoom menu, Show/Hide UI and Maximize
- a floating context bar for board items, with colour, shape, font size and connector style and caps

**Architecture:** `sawit-board` gains pure models for each of these. `toolbar.rs` builds the tool
groups, the active tool and the flyout items. `context_bar.rs` builds the controls for the current
selection and the bar's anchor. The app renders them with gpui-kit (`Button`, `Popover`, `Tooltip`, `Kbd`,
`PopupMenu`). Every click runs an engine command through `BoardDoc::execute`.

**Tech Stack:** As Plan 1. gpui-kit components in `gpui_kit::component::*`.

**Spec:** `docs/superpowers/specs/2026-10-29-native-canvas-design.md` ("Toolbar and context bar", D11)

## Global Constraints

- Toolchain: Rust 1.99.1 (`rust-toolchain.toml`), edition 2024, workspace member crates only.
- FigCraft deps are pinned in root `[workspace.dependencies]` to rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`.
- `crates/board` never depends on GPUI. All GPUI code lives in `crates/app/src/board/`.
- User-facing name stays "Canvas". Chrome uses `cx.theme()` colours and Sawit's `Lucide` icons. No FigCraft or ArtCraft artwork.
- The native canvas is reachable only when `Workspace::native_canvas` is true. Plan 8 removes this.
- Engine errors become a toast. No `unwrap`/`expect` outside tests.
- Gates before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the touched crates' tests.
- Commits:
  - Commit on `main` with explicit `git add <paths>`.
  - Conventional Commits with lowercase subjects. Every message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - **Never `git push`.**

## Review Focus

1. A selection near the canvas edge, or partly off-screen, must keep the context bar fully inside the canvas. Test: `anchor_is_clamped_inside_canvas` (Task 2).
2. A mixed selection (sticky + connector) must show only the controls both share, never a control that errors. Test: `mixed_selection_shows_shared_controls` (Task 2).
3. A tool chosen by keyboard (Q, B, X) must show as active in the toolbar on the next frame. Test: `active_tool_follows_session` (Task 1).
4. Switching a canvas tab between a board and a design file must swap tool sets, with no stale flyout open. Test: `groups_differ_by_kind` (Task 1). Task 3 Step 3 also closes any open flyout when `kind()` changes.
5. Templates placed while zoomed far out must land in view and be one undo step. Test: `template_is_one_undo_step` (Task 1).

---

### Task 1: Toolbar model

**Files:**
- Create: `crates/board/src/toolbar.rs`
- Modify: `crates/board/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum ToolIcon { Move, Hand, Scale, Frame, Section, Slice, Rectangle, Line, Arrow, Ellipse, Polygon, Star, Pen, Pencil, Text, Marker, Highlighter, Eraser, Sticky, Shapes, Connector, Stamp }
  pub struct ToolButton { pub command: &'static str, pub tool: ToolId, pub label: &'static str, pub shortcut: Option<String>, pub icon: ToolIcon }
  pub enum Flyout { StickyColors, Shapes, ConnectorStyles, Stamps }
  pub struct ToolGroup { pub key: &'static str, pub tools: Vec<ToolButton>, pub flyout: Option<Flyout> }
  pub struct FlyoutItem { pub command: &'static str, pub params: serde_json::Value, pub label: String, pub swatch: Option<[u8; 4]>, pub selected: bool }
  pub struct Template { pub id: String, pub name: String, pub description: String }
  pub fn groups(kind: DocKind) -> Vec<ToolGroup>;
  pub fn active(s: &Session) -> ToolId;                     // s.tool.tool
  pub fn flyout(s: &Session, f: Flyout) -> Vec<FlyoutItem>;
  pub fn templates(board: &mut BoardDoc) -> Result<Vec<Template>, BoardError>; // "board.templates"
  pub fn place_template(board: &mut BoardDoc, id: &str) -> Result<(), BoardError>; // "board.template" {id} at the viewport centre
  pub fn place_table(board: &mut BoardDoc, rows: u32, cols: u32) -> Result<(), BoardError>; // "board.table", params per crates/engine/src/boards/board_table.rs:36
  ```
- Groups follow the spec's lists exactly:
  - board groups: `move`, `ink`, `sticky`, `shape`, `connector`, `text`, `section`, `stamp`
  - design groups: `move`, `frame`, `shape`, `pen`, `text`
  - `shortcut` = `keymap::primary(command, Platform::Mac).map(|s| keymap::display(s, Platform::Mac))`
- Flyout contents:
  - **StickyColors:** `BOARD_COLORS`, giving `tool.sticky {color: name}`. The selected one equals
    `s.tool.board.sticky`.
  - **Shapes:** `BoardShape::ALL`, giving `tool.shape {shape}` with camelCase names.
  - **ConnectorStyles:** straight, elbow and curved, giving `tool.connector {style}`.
  - **Stamps:** `Stamp::ALL`, giving `tool.stamp {stamp}`.

- [ ] **Step 1: Write the failing tests:**
  - `groups_differ_by_kind`: Board has `tool.sticky` and no `tool.frame`; Design has `tool.frame` and no `tool.sticky`; neither has `tool.comment`.
  - `shortcuts_use_mac_glyphs`: the Move button shortcut is `Some("V")`; Section is `Some("⇧S")`. Match `keymap::display` output exactly.
  - `active_tool_follows_session`: `route_key("q")` → `active(..) == ToolId::Sticky`.
  - `sticky_flyout_marks_current_colour`: 8 items. After executing `tool.sticky {color:"blue"}`, only `blue` is selected.
  - `every_flyout_item_runs`: execute each item of every flyout on a board → all `Ok`.
  - `template_is_one_undo_step`: zoom to 0.05; `place_template` with the first id → the page has layers inside the viewport bounds; one `edit.undo` empties it.
  - `table_places_rows_and_columns`: `place_table(2, 3)` → 1 new top-level layer.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board toolbar::`
Expected: FAIL.

- [ ] **Step 3: Implement `toolbar.rs`.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board toolbar::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/toolbar.rs crates/board/src/lib.rs
git commit -m "feat(canvas): toolbar model for board and design tools"
```

### Task 2: Context bar model

**Files:**
- Create: `crates/board/src/context_bar.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Control {
      Colors { items: Vec<(&'static str, [u8; 4])>, current: Option<&'static str> }, // board.setColor {color}
      Shape { current: Option<BoardShape> },                                         // board.setStyle {shape}
      FontSize { current: Option<&'static str> },                                    // board.setStyle {fontSize}; names from board::TEXT_SIZES
      Routing { current: Option<Routing> },                                          // board.setStyle {style}
      StartCap { current: Option<String> },                                          // board.setStyle {startCap}
      EndCap { current: Option<String> },                                            // board.setStyle {endCap}
  }
  pub const CAPS: &[&str] = &["none", "lineArrow", "triangleArrow", "reversedTriangle", "circleFilled", "diamondFilled"];
  pub struct ContextBar { pub anchor: (f64, f64), pub controls: Vec<Control> }
  pub fn context_bar(s: &Session) -> Option<ContextBar>; // boards only; None with no selected board item
  pub fn clamp_anchor(anchor: (f64, f64), bar: (f64, f64), canvas: (f64, f64)) -> (f64, f64); // 8 px margin
  ```
- Which controls each board item gets (port of `ui-egui/src/canvas/board_bar.rs` `has()`):
  | Item | Controls |
  |---|---|
  | sticky | Colors, FontSize |
  | shape | Colors, Shape, FontSize |
  | connector | Colors, Routing, StartCap, EndCap |
  | ink | Colors |
- A mixed selection gets only the controls every item shares.
- `current` is `None` when the values differ.
- The anchor is the screen top-centre of `selection_box`, minus 12 px.

- [ ] **Step 1: Write the failing tests:**
  - `sticky_selection_shows_colours_and_size`: 1 sticky selected → `[Colors, FontSize]`, with `current == Some("yellow")` for the default colour.
  - `connector_selection_shows_routing_and_caps`: `[Colors, Routing, StartCap, EndCap]`; `EndCap.current == Some("triangleArrow")`.
  - `mixed_selection_shows_shared_controls`: sticky + connector → `[Colors]`, with `current == None` when the colours differ.
  - `design_doc_has_no_context_bar`: `None`.
  - `anchor_is_clamped_inside_canvas`: `clamp_anchor((5.0, -40.0), (300.0, 36.0), (800.0, 600.0)) == (158.0, 8.0)`, i.e. the bar's centre x is held at 8 + 150.
  - `controls_run`: applying each control's first option through `board.setColor` / `board.setStyle` → `Ok`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board context_bar::`
Expected: FAIL.

- [ ] **Step 3: Implement `context_bar.rs`.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board context_bar::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/context_bar.rs crates/board/src/lib.rs
git commit -m "feat(canvas): context bar model for stickies, shapes, connectors and ink"
```

### Task 3: Render the toolbar, top bar and context bar

**Files:**
- Create: `crates/app/src/board/toolbar.rs` (`fn render_toolbar(&mut BoardView, &mut Window, &mut Context<BoardView>) -> impl IntoElement`)
- Create: `crates/app/src/board/top_bar.rs`
- Create: `crates/app/src/board/context_bar.rs`
- Modify: `crates/app/src/board/mod.rs`:
  - layout: a 36 px top bar above the canvas; the toolbar absolute at the bottom centre, 16 px off the
    bottom; the context bar absolute at the clamped anchor
  - state: `open_flyout: Option<Flyout>`. Chrome hides while `ds.view.ui_hidden` is true.

**Interfaces:**
- Consumes:
  - `toolbar::{groups, active, flyout, templates, place_template, place_table}`
  - `context_bar::{context_bar, clamp_anchor, CAPS}`
  - `ds.view.ui_hidden`, which `view.toggleUi` (⌘\) flips in the engine
- Produces:
  - `fn tool_icon(ToolIcon) -> Lucide`: the nearest existing `Lucide` variant per tool. Keep a single
    `match` with no fallbacks.
  - **Top bar:**
    - Name: `"Canvas"` for the project canvas, else the file name.
    - Zoom button showing `"{zoom·100:.0}%"`. Its `PopupMenu` runs `view.zoomIn`, `view.zoomOut`,
      `view.zoomFit`, `view.zoomSelection`, `view.zoom50`, `view.zoom100` and `view.zoom200`.
    - Show/Hide UI button, which executes `view.toggleUi`.
    - Maximize button: `cx.emit(ToggleCanvasMaximize)`.
  - **Toolbar:**
    - One `Button` per tool with a tooltip reading "Label  ⌘K"-style text.
    - The active tool uses `.selected(true)`.
    - Flyout groups add a chevron button that opens a `Popover`. A colour item shows a swatch.
    - Boards get a final Templates & table button. Its popover lists `templates()`, then a 6×6 hover
      grid that calls `place_table(r, c)`.
    - When `board().kind()` changes, `open_flyout` is reset to `None`.
  - **Context bar:**
    - A row of small controls: swatches; `PopupMenu`s for shape, size, routing and caps.
    - It is hidden while dragging (`session.tool.busy()`) and while editing text.
  - While `ds.view.ui_hidden` is true, only the canvas renders.

- [ ] **Step 1: Write the failing tests** (`#[gpui_kit::test]`):
  - `toolbar_click_selects_tool`: on a `BoardView` for a new board, call the tool button's handler for `tool.sticky` (expose it as `BoardView::choose(&mut self, command, params, cx)`) → `active == Sticky`.
  - `toggle_ui_hides_chrome`: `on_key` with ⌘\ → `ds.view.ui_hidden == true`, and the rendered tree has no toolbar; again → false.
  - `flyout_closes_on_kind_change`: set `open_flyout = Some(StickyColors)`, replace the board with a design doc via `BoardView::replace_doc` (test helper), then `render` → `open_flyout == None`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit board::`
Expected: FAIL.

- [ ] **Step 3: Implement** the three views and the layout.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/board
git commit -m "feat(canvas): floating toolbar, top bar and context bar on the native canvas"
```

### Task 4: Live verification

- [ ] **Step 1:** Launch with the flag (Plan 1 Task 7), open the canvas and screenshot.
  - Expected: the board toolbar shows at the bottom centre and the top bar reads "Canvas · 100%".
- [ ] **Step 2: Check the context bar.**
  - Place a sticky through the `command` and `pointer` ops, then select it with `{"op":"command","command":"select.all"}`.
  - Expected: the screenshot shows the context bar with 8 colours above the sticky.
- [ ] **Step 3: Check the design toolbar.**
  - Copy `ridgeline-app.figcraft` to `$S/proj/.nyawit/canvas.figcraft` and reopen.
  - Expected: the design toolbar shows and there is no context bar.
- [ ] **Step 4: Ask the user to check by hand:**
  - clicking tools and flyouts
  - placing a template
  - a 3×3 table
  - zoom menu items
  - Record their answers in this plan.
- [ ] **Step 5: Stop the instance** with `/bin/kill`, then run the full gates:
Run: `cargo test --workspace`
Expected: all pass.

#### Task 4 notes: live check (2026-10-30, at `c5d882b` and lane commit `b978572`, lane 4)

Steps 1–3 passed over the control socket. So did the checks that the reviews of Tasks 2 and 3 left
to this one: the context bar's menu widths, its clamping, the marked tool button and the toast over
the toolbar. The check found one defect: a selection below the view sent the context bar under the
toolbar. It is fixed in lane commit `b978572`, on main as `4bffa2f` "fix(canvas): keep the context
bar over the toolbar" (see "The context bar under the toolbar" below). The last two runs below and
the test run were made on the lane commit, before it was rebased onto main, so these notes name it.
`cargo test --workspace` passes at lane commit `b978572`: 1136 passed, 9 ignored and 0 failed.
Step 4 needs the user: its checks are under "Hand checks (pending — needs the user)" below, with the
ones the reviews of Tasks 2 and 3 asked for. Machine: Apple M3 (8 cores, 16 GB), macOS 26.6.1,
window scale factor 2, debug build. Other lanes were building, so `vm.loadavg` was 5.1–8.3.

**How it ran.** As rulings C15–C17 amend Steps 1–3. The app was launched from lane 4 with Plan 1
Task 7's launch recipe for an empty board (no `.nyawit/`, C15), and stopped with `/bin/kill $PID`
each time (C17 as amended). It ran three times:
- Steps 1–2 and the board checks, at `c5d882b`;
- the fix, on the same board, at lane commit `b978572` (on main as `4bffa2f`);
- Step 3, with `ridgeline-app.figcraft` copied over `.nyawit/canvas.figcraft` before the launch, at
  lane commit `b978572`. The fix touches nothing a design file shows.

Another app's window covered Sawit's at times, and a covered window doesn't repaint, so each batch
of screenshots started with `{"cmd":"focus"}`. After Step 1, `{"cmd":"layout","terminal":false}`
gave the canvas 1040 × 675 pt. Pointer ops take page points.

**Answers (Steps 1–3).**

- Step 1:
  - `open_page canvas` answers `{"tab":0}`, and `get` answers `[]`.
  - The screenshot shows the board toolbar at the bottom centre of the 1040 × 435 pt canvas: Move,
    Hand | Marker, Highlighter, Eraser | Sticky note ⌃ | Shapes ⌃ | Connector ⌃ | Text | Section |
    Stamp ⌃ | Templates & table. It is 508 × 44 pt and 16 pt off the canvas's bottom edge, and its
    middle is within 1 pt of the canvas's.
  - The Move button is marked: a 26 × 26 pt square of `#454a56` on the toolbar's `#2f343e`.
  - The top bar reads "Canvas" and "100%" side by side (C19), with a caret after the zoom. The Hide
    UI and Maximize buttons are at its right.
- Step 2, with Escape before `select.all` (C16):
  - `tool.sticky` answers `{"tool":"sticky"}`. `pointer` down and up at (0, 0) answer
    `selection: ["41"]` (the sticky's text, now being typed in) and `tool: "move"`.
  - A screenshot then shows the sticky's text box with its caret, and no context bar: the bar hides
    while text is edited.
  - `key Escape` answers `handled: true`, and `select.all` answers `{"selection":[40]}`.
  - The screenshot shows the context bar over the sticky: 8 swatches (yellow, orange, red, pink,
    violet, blue, green and gray), yellow marked. It is 220 × 40 pt. Its middle is over the
    sticky's, and its bottom 12 pt over the sticky's top.
- Step 3, reopened as C17 (amended) says:
  - `ridgeline-app.figcraft` was copied over `.nyawit/canvas.figcraft` (same SHA-1) before the third
    launch. `open_page canvas` answers `{"tab":0}`. `get` answers its 4 layers: `"349"` Components,
    `"68"` Home — Light, `"239"` Home — Dark and `"178"` Trail — Light.
  - The screenshot shows the design toolbar: Move, Scale, Hand | Frame, Section, Slice | Rectangle,
    Line, Arrow, Ellipse, Polygon, Star | Pen, Pencil | Text. It has no chevrons and no Templates &
    table button, and it is 476 pt wide. The top bar reads "Canvas" and "28%".
  - There is no context bar, and still none once `select.all` has selected all 4 layers
    (2639 × 1116).
  - Key R marks Rectangle. The file was still byte-identical after the run.

**Also checked over the socket.** The board held the sticky `"40"`, a rectangle `"42"` reading
"Shape", a connector `"45"` from the sticky to it, and an ellipse `"46"` reading "Big" in XL text.

- **The context bar for each kind** (Review Focus 2):
  - The rectangle: the swatches, yellow marked, then the "Rectangle" and "M" menus.
  - The connector: the swatches, none marked (its line, `#3A3936`, is no board colour's), then
    "Elbow", "None" and "Triangle arrow".
  - The sticky and the connector together: the swatches alone, none marked.
  - The rectangle and the ellipse together: yellow marked, then "Mixed" and "Mixed".
  - All four: the swatches alone.
  - The longest labels, on a second board (a second project opened in the window with
    `{"cmd":"open"}`): "Rounded rectangle" and "Speech bubble" in the 160 pt shape menu, "Curved" in
    the 92 pt style menu, and "Reversed triangle" in both 156 pt cap menus. Every label shows whole,
    and so does "Mixed" in the 76 pt size menu (Task 3's fix).
- **Clamping** (Review Focus 1):
  - The view was moved so that the selected sticky's top was 20 pt under the canvas's top and its
    middle on the canvas's left edge (`view.set {cx: 500, cy: 217.5}`). The bar is 8 pt from the
    canvas's left and top edges. It covers the sticky's top, as the model's clamp pins
    (`anchor_is_clamped_inside_canvas`).
  - With the sticky's middle on the right edge instead, the bar is 8 pt from the right and top
    edges.
  - The rectangle's bar, wider than the room right of the rectangle, is held 8 pt from the right
    edge.
- **A drag hides the bar.** `pointer` down on the sticky and two moves, with no up: the bar is gone
  while the sticky moves. After the up it is back, 12 pt over the moved sticky. `edit.undo` put the
  sticky back.
- **Tool keys** (Review Focus 3): keys Q, B, X and V each mark their tool's button (Sticky note,
  Shapes, Connector, Move) in the next screenshot. On the design file, R marks Rectangle.
- **⌘\\** (`{"op":"key","key":"\\","cmd":true}`) answers `handled: true`. The top bar, the toolbar
  and the context bar go, and the canvas takes their room. The selection's outline, handles and size
  stay: they are the canvas's own. ⌘\ again brings all three back.
- **The zoom button** follows the zoom menu's commands: `view.zoom50` reads "50%", `view.zoom200`
  "200%", `view.zoomFit` (zoom 1.334) "133%" and `view.zoom100` "100%". `view.zoomSelection`,
  `view.zoomIn` and `view.zoomOut` answered zooms 2.735, 4 and 2.
- **Templates** (Review Focus 5):
  - `board.templates` lists 5: retrospective, brainstorm, weeklyPlanner, flowChart and kanban.
  - At zoom 0.05, `board.template {name: "flowChart"}` places its 12 layers in the middle of the
    view, selected (1220 × 360, at "5%"), with a colour bar over them. One `edit.undo` removes them
    all.
  - kanban at 60 % lands in the middle of the view too: its title and 3 sections, selected, and no
    bar. Sections and text take no control (P3-C6).
- **A 3 × 3 table:** `board.table {rows: 3, columns: 3}` answers 3 × 3 cells, and `get` lists one
  new top-level layer, `"49"`, a table. The screenshot shows it in the middle of the view, 480 × 122,
  selected, its header row shaded. There is no bar: tables take no control.
- **The toast over the toolbar** (P3-C10): `edit.pickColor`, then a click on the sticky, shows the
  toast "Picked #FFE78A", its bottom 8 pt over the toolbar's top.
- **After a restart**, the board opened as it was left, zoomed to fit (73 %).
- **The logs** hold only `canvas render` lines, 60, 4 and 6 in the three runs. There are no warnings
  or errors.
- **The board changed with no op of this check behind it, most likely by hand.** In the first run,
  between Step 1's screenshot (18:17:20) and Step 2's first one (18:18:34), the board got FigCraft's
  Flow chart template and was emptied again, and the zoom went from 100 % to 34 %. `.nyawit/`
  appeared at 18:17:42, with the first save after an edit, and `canvas.figcraft.bak` (written
  18:18:21) holds the flow chart board.
  - No op of this check did it. The template alone doesn't say who did: the Templates & table
    picker places one, and so does `board.template` from any client of the control socket, as this
    check's own b01 did.
  - Nothing else is known to have used this instance's socket. It was the per-`SAWIT_HOME`
    fallback `/tmp/sawit-rulisme-945615192beed5f.sock`, which sawit-mcp and the `sawit` CLI never
    use without that `SAWIT_HOME` (`AppPaths::default_macos` and `cli_socket`,
    crates/core/src/instances.rs:20-28, :62-72). No other live check ran then: the run's others
    ended at 06:12 (Plan 1) and 17:26 (Plan 2), and FINAL.P2's began at 19:00.
  - 34 % is no step of the zoom menu or keys: Zoom in and Zoom out go to the next of 2, 3, 6, 13,
    25, 50, 100, 200 % and up, and the presets are 50, 100 and 200 % (engine `cmd/view.rs`,
    `STEPS`). Nor is it a fit of the 1220 × 360 flow chart on the 1040 pt wide canvas, which is
    about 75 % with the engine's 64 pt margin (`ViewState::fit`). It is what a pinch or a ⌘-scroll
    leaves.
  - The window lost key focus in between: its title bar is active in s1 and inactive in s2a.
  - So most likely a person at the machine used the window. The user hasn't confirmed it yet.
  - The verdicts don't rest on that board: the view was set back to 100 % around (0, 0) before
    Step 2's screenshot. The template's layers had taken the ids before it, which is why Step 2's
    sticky is `"40"`.
- **The context bar under the toolbar (a defect, fixed in lane commit `b978572`, on main as
  `4bffa2f`).**
  - With the sticky selected, the view was moved until the sticky was below the canvas
    (`view.set {cx: 0, cy: -600}`). The bar went to the canvas's bottom edge, under the toolbar,
    which covered all of it but a 7.5 pt strip below the toolbar's own bottom edge.
  - The cause: the view clamped the bar inside the whole canvas. The toolbar floats over the
    canvas's bottom 60 pt and is drawn over the bar. FigCraft's own bar stays clear of its toolbar
    (ui-egui `board_bar.rs`, `place`).
  - The fix: the view clamps the bar in the part of the canvas above the toolbar, so it stops 8 pt
    over the toolbar, as the toast does (P3-C10). The model's clamp (`clamp_anchor`) is unchanged.
    Test: `the_context_bar_stays_over_the_toolbar`.
  - After a rebuild, the same view put the whole bar 8 pt over the toolbar. A selection in view keeps
    its bar 12 pt over it.
- **A narrow canvas** (Task 3's review; decided during the run by the delegate's ruling on this
  task: the toolbar follows FigCraft's, fixed after this check in "fix(canvas): keep the toolbar
  inside a narrow canvas and scroll its buttons"):
  - The window can't be resized over the socket. Its smallest size is 720 × 480 pt, and Sawit's
    sidebar is 240 pt wide.
  - At `c5d882b` and lane commit `b978572` (on main as `4bffa2f`) the toolbars didn't wrap or
    scroll. Centred at their full width, 508 pt on a board and 476 pt on a design file, they were
    cut evenly at both ends of a narrower canvas. The 480 pt canvas of the narrowest window with
    Sawit's sidebar shown cut 14 pt off each end of the board toolbar. With Plans 4 and 5's panels
    (240 + 264 pt) beside a design file, the narrowest window without the sidebar leaves 216 pt,
    which would cut 130 pt off each end: whole tools out of the pointer's reach.
  - The fix ports FigCraft's rule (ui-egui `toolbar.rs`, `pill_rect` and `pill_scroll`). The
    toolbar keeps its full width while the canvas has room for it and 8 pt at each side: 524 pt
    for a board, 492 pt for a design file. It is never wider than the canvas less 16 pt. Narrower,
    its buttons, chevrons and lines keep their size and scroll sideways inside it, under a
    two-finger swipe or the wheel. It still takes the pointer: no click or scroll over it reaches
    the canvas.
  - What that makes of the cases above, computed from the measured widths (they can't be measured
    over the socket): in the 480 pt canvas the board toolbar is 464 pt wide and scrolls by up to
    44 pt, and the Templates & table button and the line before it start out of sight. A 1280 pt
    window has room for the whole design toolbar beside both panels: 536 pt with the sidebar shown.
    In the 216 pt canvas the design toolbar is 200 pt wide and shows about two fifths of its
    buttons at a time.
  - Test: `the_toolbar_scrolls_inside_a_narrow_canvas`. Hand check 6 is how it looks and feels.
  - After this check, Plan 3's final review found the same gap in the context bar. It kept its full
    width, 474 pt for a shape and 651 pt for a connector, so a narrower canvas cut it at the right,
    where a connector's End cap menu is: at the 480 pt canvas, 179 pt of it. It now follows the
    toolbar's rule: it is never wider than the canvas less 16 pt, and its controls keep their size
    and scroll sideways inside it. Computed from its widths in the code, a connector's bar in the
    480 pt canvas is 464 pt wide and scrolls by up to 187 pt. Test:
    `the_context_bar_scrolls_inside_a_narrow_canvas`. Hand check 6 covers it too.

#### Hand checks (pending — needs the user)

Launch with Plan 1 Task 7's launch recipe for an empty board (skip the line that copies the example),
and run its `open_page` line. Click the canvas once, so that it has the keyboard. Where a check
already passed over the socket, it says so.

1. **Tools and flyouts** (Step 4, and Task 3's review).
   - Click each tool button. It shows marked (a lighter square), and its tool is on: after Sticky
     note, a click on the canvas places a sticky. Hover a button: its tooltip reads its name and key
     ("Sticky note  Q").
   - Press Q, B, X and V: the marked button follows at once. This passed over the socket.
   - Click the chevron beside Sticky note. The flyout opens above the toolbar, centred on the
     chevron: 8 swatches, the current colour marked. Pick blue: the flyout closes, the Sticky note
     tool is on, and the next sticky is blue.
   - The chevron beside Shapes lists the 7 shapes, the current one checked. Pick Ellipse: the
     Shapes tool now places ellipses. The Connector chevron lists Straight, Elbow and Curved, and
     the Stamp chevron the 6 stamps.
   - Press Escape first if a sticky's text has a caret: ⌘\ does nothing while text is typed in.
     Open a flyout and press ⌘\: nothing happens, because the open flyout has the keyboard (the
     canvas takes only the keys pressed while it has the keyboard itself, ruling P5-C3). Press
     Escape: the flyout closes, and the canvas has the keyboard again. Then press ⌘\ twice: the
     chrome goes, then comes back with no flyout open.
   - Say whether the marked look of a tool button is clear enough.
2. **Placing a template** (Step 4, and Review Focus 5).
   - Click the last button, Templates & table. The picker opens above the toolbar: Retrospective,
     Brainstorm, Weekly planner, Flow chart and Kanban board, each with a line of description, then
     "Table" and a 6 × 6 grid.
   - Zoom far out (the zoom menu's 50 %, then ⌘− a few times), open the picker, and click Flow
     chart. It lands in the middle of the view, selected, and the picker closes. One ⌘Z removes all
     of it. Over the socket, the same command at 5 % did this.
   - Open the picker, and leave it open for 10 s. Activity Monitor shows Sawit at about 0 % CPU:
     the list isn't asked for on every frame (P3-C14).
3. **A 3 × 3 table** (Step 4).
   - Open the picker, and move the pointer over the grid. The cells from the top left to the one
     under the pointer light up, and over the third cell of the third row the heading reads
     "Table  3 × 3".
   - Click it. A 3 × 3 table lands in the middle of the view, selected, and the picker closes. ⌘Z
     removes it. Over the socket, `board.table` placed the same table.
4. **The zoom menu** (Step 4).
   - Click "100%" in the top bar. The menu lists Zoom in and Zoom out, then Zoom to fit and Zoom to
     selection, then 50 %, 100 % and 200 % (the engine's labels), each with its keys on the right.
   - Each item does what it says, and the button then shows the new zoom. Over the socket, the same
     commands changed the label.
5. **The context bar** (Task 3's review, and Review Focus 2).
   - Select a sticky, and click the blue swatch. The sticky turns blue, and that swatch is marked.
   - Select a shape, and open its shape menu: the 7 shapes, the current one checked. Pick the checked
     one: nothing happens, and no toast shows (P3-C2). Pick Diamond: the shape becomes a diamond.
     The size menu offers S, M, L and XL.
   - Select a connector. Its style and cap menus change it.
   - Each menu opens by its button, and the bar's buttons look right when hovered and when marked.
6. **A narrow canvas** (Task 3's review and Plan 3's final review; the toolbar scrolls, as FigCraft's
   does, and so does the context bar).
   - With Sawit's sidebar shown, drag the window's edge in until it stops (720 pt wide). The canvas
     is then about 480 pt wide.
   - Expect the board toolbar 8 pt in from both sides of the canvas, its buttons at their usual
     size, the Move button whole at its left end. The Templates & table button is out of sight past
     its right end.
   - Swipe sideways over the toolbar with two fingers, or turn a mouse wheel over it. The buttons
     slide inside it until the Templates & table button shows whole, and the canvas under it
     doesn't move. Click that button: the picker opens above it. Swipe back: the Move button
     shows again.
   - Select a connector (draw one with the Connector tool if the board has none). Its context bar
     is 8 pt in from both sides of the canvas, its colours whole at its left end, and its End cap
     menu out of sight past its right end. Swipe sideways over the bar, or turn a mouse wheel over
     it: its controls slide until the End cap menu shows whole, and the canvas under it doesn't
     move. Open that menu and pick Circle: the connector's end becomes a filled circle.
   - Widen the window. From a canvas of about 524 pt, the toolbar has its full width again, every
     button in sight. From about 667 pt, so does the connector's context bar, every control in
     sight.
   - Say whether the scrolling toolbar and context bar work well enough, or whether they should
     wrap or shrink instead.
7. **Hide UI and Maximize** (Task 3).
   - Hover the eye-off button at the top bar's right: "Hide UI  ⌘\". Click it: all the chrome goes.
     ⌘\ brings it back. The button only hides (P3-C12).
   - Hover the Maximize button: "Maximize  ⇧⌘↩". Click it: Sawit's sidebar and terminal go, and the
     canvas takes the window. Click it again: they come back.

Write the answers here once the checks have run (R5).
