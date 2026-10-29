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
