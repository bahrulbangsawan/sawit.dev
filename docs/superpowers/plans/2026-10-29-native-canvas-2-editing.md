# Native canvas 2: Editing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The native canvas becomes editable: pointer tools, selection and handle overlays, cursors, the FigCraft keymap under Sawit's conflict policy, on-canvas text with IME, a clipboard shared between canvas tabs, and the `pointer`/`key`/`type` control ops.

**Architecture:** Input decisions live in `sawit-board`, which drives the engine directly:
- `input.rs` routes keys, as a port of FigCraft's `Headless::key`
- `overlay.rs` builds a display list of shapes in screen coordinates
- `clipboard.rs` holds the cross-tab clipboard and the paste policy
- `policy.rs` holds the hidden and UI-handled command ids

The app's `BoardView` only translates GPUI events into these calls, paints the display list, and
implements `EntityInputHandler` for IME.

**Tech Stack:** As Plan 1. FigCraft APIs used:
- `Session::{pointer, pointer_left, tool_key, escape}`
- `figcraft_engine::{tools, textedit, keymap, guard}`
- `figcraft_text::layout_node`

**Spec:** `docs/superpowers/specs/2026-10-29-native-canvas-design.md` (sections D4, D7, D15, Keyboard conflict policy, Agent wire contract)

## Global Constraints

- Toolchain: Rust 1.99.1 (`rust-toolchain.toml`), edition 2024, workspace member crates only.
- FigCraft deps are pinned in root `[workspace.dependencies]` to rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`. Never use a branch or an unpinned dependency.
- `crates/board` never depends on GPUI. All GPUI code lives in `crates/app/src/board/`.
- User-facing name stays "Canvas".
- The native canvas is reachable only when `Workspace::native_canvas` is true (env `SAWIT_NATIVE_CANVAS`). Plan 8 removes this.
- Engine errors become a toast or an `{ok:false,error}` reply. Engine calls other than `execute` go through `figcraft_engine::guard::run`.
- Gates before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the touched crates' tests.
- Commits:
  - Commit on `main` with explicit `git add <paths>`.
  - Conventional Commits with lowercase subjects. Every message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - **Never `git push`.**

## Review Focus

1. A drag that leaves the canvas, or the window, must still end the gesture with one undo step and no stuck tool. Tests: `drag_released_outside_ends_gesture` (Task 2) and the window-level mouse-up listener in Task 2 Step 3.
2. Keys that FigCraft and Sawit both want must follow the conflict table exactly, both while editing text and while not. Tests: `board_keys_reach_the_canvas`, `sawit_keeps_its_shortcuts`, `text_editing_takes_text_keys` (Task 4).
3. An IME composition cancelled with Escape, or by clicking away, must leave no preedit text in the document. Test: `cancelled_composition_leaves_no_text` (Task 5).
4. Hidden commands (e.g. `file.new` on ⌘N, `edit.pickColor` on I) must do nothing from the keyboard. Test: `hidden_commands_are_blocked` (Task 1).
5. Paste with an empty clipboard, or after the user copied text elsewhere, must not error out loudly. It pastes that text instead. Test: `paste_prefers_newer_system_text` (Task 6).

---

### Task 1: Command policy and key routing

**Files:**
- Create: `crates/board/src/policy.rs`
- Create: `crates/board/src/input.rs`
- Modify: `crates/board/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  // policy.rs: lists copied from the spec's Menu policy
  pub const HIDDEN: &[&str];                        // exact ids; plus prefixes below
  pub const HIDDEN_PREFIXES: &[&str] = &["plugins.", "plugin.", "libraries."];
  pub const UI_COMMANDS: &[&str];                   // the spec's UI-handled ids
  pub fn is_hidden(id: &str) -> bool;
  pub fn is_ui(id: &str) -> bool;
  // GPUI keystrokes that Sawit keeps while the canvas has focus (spec: Keyboard conflict policy)
  pub const SAWIT_WINS: &[&str] = &["cmd-w", "cmd-o", "ctrl-tab", "ctrl-shift-tab", "ctrl-cmd-m", "alt-cmd-o", "cmd-j", "shift-cmd-j"];
  pub const SAWIT_WINS_UNLESS_EDITING: &[&str] = &["cmd-b", "alt-cmd-t", "shift-cmd-enter"];

  // input.rs
  pub use figcraft_engine::keymap::Mods;
  pub enum KeyOutcome { Handled, Ui(&'static str), Unhandled }
  pub fn key_name(gpui_key: &str) -> Option<String>;   // GPUI Keystroke::key → FigCraft key name
  pub fn gpui_keystroke(spec: &str) -> Option<String>; // FigCraft "Shift+Cmd+K" → GPUI "shift-cmd-k"
  pub fn route_key(board: &mut BoardDoc, key: &str, mods: Mods) -> Result<KeyOutcome, BoardError>;
  ```
- `key_name` mapping:
  - backspace → `Backspace`; delete → `Delete`
  - left, right, up, down → `ArrowLeft`, `ArrowRight`, `ArrowUp`, `ArrowDown`
  - enter → `Enter`; escape → `Escape`; tab → `Tab`; space → `Space`
  - home → `Home`; end → `End`; pageup → `PageUp`; pagedown → `PageDown`
  - f1–f12 → `F1`–`F12`
  - a single character stays as it is
  - anything else → `None`
- `gpui_keystroke` does the inverse:
  - modifiers in the order ctrl, alt, shift, cmd
  - `Delete` → `backspace`
  - shifted aliases resolved through `keymap::parse`, e.g. `Plus` → `shift-=`
- `route_key` ports `crates/mcp/src/headless.rs:128-200` (`Headless::key`) verbatim, with three changes:
  - **Hidden ids:** a binding whose id `is_hidden` is treated as not found (→ `Unhandled`).
  - **UI ids:** a binding whose id `is_ui` returns `Ui(id)` instead of executing.
  - **Typing:** while text editing, a single printable character without cmd/ctrl returns `Unhandled`,
    so the IME path inserts it (Task 5).

- [ ] **Step 1: Write the failing tests** (`input.rs`, `policy.rs`):
  - `key_names_map_gpui_to_figcraft`: `backspace→Backspace`, `left→ArrowLeft`, `k→k`, `f5→F5`, `capslock→None`.
  - `gpui_keystrokes_from_specs`: `"Shift+Cmd+K"→"shift-cmd-k"`, `"Cmd+\\"→"cmd-\\"`, `"Delete"→"backspace"`, `"Plus"→"shift-="`.
  - `v_selects_move_tool`: after `tool.sticky`, `route_key("v", none)` → `Handled` and the tool id is `move`.
  - `undo_runs_through_the_keymap`: create a sticky, `route_key("z", cmd)` → `Handled` and the page is empty.
  - `hidden_commands_are_blocked`: `route_key("n", cmd)` → `Unhandled`; `route_key("i", none)` → `Unhandled`; the docs count is still 1.
  - `ui_commands_are_returned`: `route_key("s", cmd)` → `Ui("file.save")`.
  - `ui_requests_are_left_for_the_view`: `route_key("k", cmd)` → `Handled`, and `session.ui_requests` holds kind `"palette.actions"`. `route_key("\\", cmd)` → `Handled`, and `ds.view.ui_hidden` is true.
  - `typing_is_left_to_the_ime_while_editing`: make a sticky and `textedit::begin` it; `route_key("a", none)` → `Unhandled`; `route_key("Backspace", none)` → `Handled`.
  - `policy_lists_match_spec`: `is_hidden("file.new")`, `is_hidden("plugins.manage")`, `!is_hidden("edit.copy")`, `is_ui("file.export")`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board input:: policy::`
Expected: FAIL (missing modules).

- [ ] **Step 3: Implement `policy.rs` and `input.rs`.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board input:: policy::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/policy.rs crates/board/src/input.rs crates/board/src/lib.rs
git commit -m "feat(canvas): route canvas keys through figcraft's keymap with sawit's command policy"
```

### Task 2: Pointer input and cursors

**Files:**
- Modify: `crates/board/src/input.rs` (pointer helpers)
- Modify: `crates/board/src/ops.rs` (`pointer` op)
- Create: `crates/app/src/board/events.rs` (mouse handlers)
- Modify: `crates/app/src/board/mod.rs`

**Interfaces:**
- Consumes: `BoardDoc`, `view::*`.
- Produces:
  ```rust
  // sawit-board
  pub fn pointer_event(kind: PointerKind, local: (f64, f64), view: &ViewState, mods: Mods) -> PointerEvent; // page coords via screen_to_page; zoom = view.zoom
  pub fn pointer(board: &mut BoardDoc, ev: &PointerEvent) -> Result<Vec<UiRequest>, BoardError>; // guard::run + drain session.ui_requests
  pub fn handle_requests(board: &mut BoardDoc, reqs: &[UiRequest]) -> Vec<UiRequest>; // performs "board.editText" (textedit::begin on data.id); returns the rest for the view
  pub enum ResizeDir { Horizontal, Vertical, DiagDown, DiagUp }
  pub fn resize_dir(angle_radians: f64) -> ResizeDir;  // nearest of 0°, 90°, 45°, 135° (mod 180°)
  // ops.rs: "pointer" {kind: down|move|up|double|hover, x, y (page), shift?, alt?, cmd?} → {ok, selection, tool}
  // app
  pub(super) fn cursor_style(c: &figcraft_engine::tools::Cursor) -> CursorStyle;
  ```
- Cursor table:
  | FigCraft `Cursor` | GPUI `CursorStyle` |
  |---|---|
  | Default, Move, Comment | Arrow |
  | Crosshair, Pen, Eyedropper, New, Rotate | Crosshair |
  | Resize(a) | `resize_dir(a)` → ResizeLeftRight / ResizeUpDown / ResizeUpLeftDownRight / ResizeUpRightDownLeft |
  | Grab | OpenHand |
  | Grabbing | ClosedHand |
  | Text | IBeam |
  | Copy | DragCopy |
  | Pointer | PointingHand |
  | NotAllowed | OperationNotAllowed |
- Events (the pattern is `db/diagram.rs:830-866`, plus `terminal/view.rs:600` for window-level drags):
  - **Left mouse down:** focus the view, then send `Down`. `click_count == 2` sends `DoubleClick` instead.
  - **Move:** `Move` while a button is down, else `Hover`.
  - **Up / up-out:** `Up`.
  - **During a drag:** a `window.on_mouse_event` listener, registered in paint, catches the move and up
    events outside the bounds.
  - **Leaving the canvas:** `Session::pointer_left`.
  - **Right button:** reserved for Plan 6.
  - **After each call:** `cx.notify()`.

- [ ] **Step 1: Write the failing tests** (`sawit-board`):
  - `pointer_event_uses_page_coordinates`: view `cx=100, cy=50, zoom=2, 800×600`, local `(400,300)` → `(x,y) == (100,50)`, `sx == 400`, `zoom == 2`.
  - `rectangle_tool_draws_one_undo_step`: design doc, `tool.rectangle`, then pointer `Down (0,0)` → `Move (50,40)` → `Up (100,80)`. Expect 1 layer of 100×80; one `edit.undo` empties the page.
  - `drag_released_outside_ends_gesture`: the same drag ending at `(5000,5000)` → 1 layer, and `session.tool.busy()` is false.
  - `sticky_click_starts_text_editing`: `tool.sticky`, then `Down` and `Up` at `(0,0)`, then `handle_requests` → `ds.text_edit.is_some()`.
  - `resize_dir_buckets_angles`: `0→Horizontal`, `π/2→Vertical`, `π/4→DiagDown`, `3π/4→DiagUp`, `π→Horizontal`.
  - `pointer_op_drives_tools`: `{op:"command",command:"tool.sticky"}`, then `{op:"pointer",kind:"down",x:0,y:0}` and `{op:"pointer",kind:"up",x:0,y:0}`. A following `get` shows 1 element.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board input:: ops::`
Expected: FAIL.

- [ ] **Step 3: Implement** the helpers, the op, `events.rs` and `cursor_style`. Apply `cursor_style(&session.tool.overlay.cursor)` with `.cursor(..)` on the canvas div.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board && cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src crates/app/src/board
git commit -m "feat(canvas): pointer tools and cursors on the native canvas"
```

### Task 3: Overlay display list and painting

**Files:**
- Create: `crates/board/src/overlay.rs`
- Create: `crates/app/src/board/overlay_paint.rs`
- Modify: `crates/app/src/board/paint.rs`

**Interfaces:**
- Produces:
  ```rust
  pub type Rgba = [u8; 4];
  pub const SELECTION: Rgba = [0x0D, 0x99, 0xFF, 0xFF];
  pub const COMPONENT: Rgba = [0x97, 0x47, 0xFF, 0xFF];
  pub const GUIDE: Rgba = [0xF2, 0x48, 0x22, 0xFF];
  pub enum Shape {
      Outline { points: Vec<(f64, f64)>, closed: bool, color: Rgba, width: f64, dashed: bool },
      Fill { points: Vec<(f64, f64)>, color: Rgba },
      Handle { center: (f64, f64), size: f64, round: bool, fill: Rgba, stroke: Rgba },
      Label { at: (f64, f64), text: String, color: Rgba, background: Option<Rgba>, node: Option<NodeId> },
      Caret { top: (f64, f64), bottom: (f64, f64), color: Rgba },
  }
  pub fn overlay(s: &Session) -> Vec<Shape>;   // screen logical px relative to the canvas origin
  pub fn label_quad(top_left: (f64, f64), size: (f64, f64), view: &ViewState) -> [figcraft_geom::Point; 4]; // screen → page quad for tool.labels
  ```
- `overlay` is a port of `ui-egui/src/canvas/overlays.rs`, `handles.rs`, `frame_labels.rs` and
  `guides.rs`, limited to the items below, in z order:
  1. Frame and section name labels: `Label` with `node`.
  2. The hover outline, when `ds.hover` is not selected.
  3. One outline per selected layer, using `COMPONENT` for components and instances.
  4. The selection box (`tools::selection_box`) and 4 corner handles. Connectors get 2 round end handles instead.
  5. The size label `"{w} × {h}"` below the selection box (integers, `SELECTION` background).
  6. The marquee: fill at 10 % alpha plus an outline.
  7. Snap guides from `overlay.guides`, with their labels.
  8. `drop_target`: an outline. `insert_line`: a 2 px line.
  9. Pen, rubber and brush previews: polylines.
  10. Vector-edit points and handles (`ds.vector_edit`, `overlay.pen_handles`, `overlay.vector_hover`).
- Not ported (the spec's non-goals or later work): face, builder_regions, lasso, measure_box, scale, arc,
  shape handles, drop_cell, swap_target, comments, rulers, spacing.
- The app paints each `Shape` with `paint_path` / `paint_quad`. Labels use `window.text_system().shape_line`.
- After painting, the app writes `session.tool.labels` from each frame `Label`'s measured box through
  `label_quad`.

- [ ] **Step 1: Write the failing tests** (`overlay.rs`, on a design doc with a frame plus a rectangle):
  - `nothing_selected_draws_only_frame_labels`: the only `Label` has `node == Some(frame)`.
  - `one_selected_layer_has_box_and_four_handles`: select the rectangle → one closed `Outline` in `SELECTION` and 4 `Handle`s at the box corners in screen px (zoom 2 doubles the distances).
  - `size_label_reads_w_by_h`: a 100×80 rectangle → a `Label` with text `"100 × 80"`.
  - `hover_outline_only_when_not_selected`: hover is the selected node → no extra outline; another node → one extra.
  - `marquee_draws_fill_and_outline`: drag with the move tool from empty space without releasing → one `Fill` and one `Outline`.
  - `connector_has_round_end_handles`: a board with 2 shapes and a selected connector → 2 handles with `round == true`.
  - `label_quad_round_trips_through_view`: at zoom 2 the quad is half the screen size.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board overlay::`
Expected: FAIL.

- [ ] **Step 3: Implement** `overlay.rs`, `overlay_paint.rs` and the labels write-back.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board overlay::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/overlay.rs crates/board/src/lib.rs crates/app/src/board
git commit -m "feat(canvas): selection, handles, guides and labels drawn over the native canvas"
```

### Task 4: Keyboard: contexts, conflict policy, `key` op

**Files:**
- Modify: `crates/app/src/board/mod.rs`:
  - `pub fn bindings() -> Vec<KeyBinding>`, registered in `init`
  - `.key_context(..)`, `.track_focus(..)`, `.on_key_down(..)`
- Modify: `crates/board/src/ops.rs` (`key` op)

**Interfaces:**
- Consumes: `input::{key_name, route_key, gpui_keystroke, KeyOutcome}`, `policy::{SAWIT_WINS, SAWIT_WINS_UNLESS_EDITING, is_hidden}`.
- Produces:
  ```rust
  pub fn bindings() -> Vec<KeyBinding>;  // NoAction in "Board" / "BoardText"
  pub(super) fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>); // key_name + route_key; Handled|Ui → stop_propagation
  // ops.rs: "key" {key, shift?, alt?, cmd?, ctrl?} → {ok, handled}
  ```
- Key context:
  - The root div has key context `Board`, and `Board BoardText` while `ds.text_edit.is_some()`.
- "Board" bindings, each to `NoAction`:
  - `cmd-0`
  - `cmd-k`, plus the workspace's `cmd-k e|t|u|w|shift-enter` chords
  - every gpui-kit / `Root` binding that a FigCraft Canvas key uses. The test in Step 1 finds these.
- "BoardText" bindings, each to `NoAction`: `cmd-b`, `alt-cmd-t`, `shift-cmd-enter`.
- After every key, the view drains `session.ui_requests` through `handle_requests` (Task 2).
  - The remaining kinds are `panel.layers`, `panel.design`, `palette.actions` and `panel.shortcuts`.
    The view ignores them until Plans 4, 5 and 6 add their panels.
- `Ui(id)` outcomes show a toast reading "Coming soon" until Plan 6 implements them.

- [ ] **Step 1: Write the failing tests** (`#[gpui_kit::test]`, binding-resolution pattern from `terminal/view.rs` tests):
  - `board_keys_reach_the_canvas`: for each `keymap::KEYS` binding live in Canvas whose id is not hidden, and each Mac spec whose `gpui_keystroke` is not in `SAWIT_WINS ∪ SAWIT_WINS_UNLESS_EDITING`, resolve the keystroke in context stack `[Workspace, Board]` with `crate::workspace::bindings()`, gpui-kit's bindings and `board::bindings()` loaded. Expect no action other than `NoAction`.
  - `sawit_keeps_its_shortcuts`: each `SAWIT_WINS` keystroke still resolves to its workspace action in `[Workspace, Board]`.
  - `text_editing_takes_text_keys`: `cmd-b`, `alt-cmd-t` and `shift-cmd-enter` resolve to `NoAction` in `[Workspace, Board BoardText]`, and to workspace actions in `[Workspace, Board]`.
  - `key_op_routes_like_the_keyboard` (`sawit-board`): `{op:"key", key:"r"}` → `handled:true`, and the tool is `rectangle`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit board:: && cargo test -p sawit-board ops::`
Expected: FAIL.

- [ ] **Step 3: Implement** the bindings, context, `on_key` and the op. Extend `NoAction` until `board_keys_reach_the_canvas` passes, and leave a one-line comment naming each Root key you added.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit && cargo test -p sawit-board`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/board crates/board/src/ops.rs
git commit -m "feat(canvas): figcraft shortcuts on the native canvas, sharing keys with sawit by policy"
```

### Task 5: On-canvas text and IME

**Files:**
- Create: `crates/app/src/board/text_input.rs` (`impl EntityInputHandler for BoardView`)
- Modify: `crates/board/src/overlay.rs` (caret, selection rects, preedit underline)
- Modify: `crates/board/src/ops.rs` (`type` op)
- Modify: `crates/app/src/board/mod.rs`:
  - register `window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx)` in paint while editing
  - render jobs use `textedit::preedit_doc` while composing

**Interfaces:**
- Consumes: `figcraft_engine::textedit::{begin, end, insert, insert_ime_commit, key, preedit, preedit_doc, ime_surrounding, delete_surrounding_chars, selected_range}`, `figcraft_text::layout_node`.
- Produces:
  - `EntityInputHandler`:
    | Method | Engine call |
    |---|---|
    | `text_for_range` / `selected_text_range` / `marked_text_range` | from `ime_surrounding` and `ds.text_edit`, converting UTF-8 bytes ↔ UTF-16 |
    | `replace_text_in_range` | `insert_ime_commit` when a preedit exists, else `insert`; a non-empty range before the caret is first removed with `delete_surrounding_chars` |
    | `replace_and_mark_text_in_range` | `preedit(text, cursor)` |
    | `unmark_text` | commits the preedit text with `insert_ime_commit` |
    | `bounds_for_range` | the caret rectangle in window coordinates |
    | `character_index_for_point` | `None` |
  - Overlay additions:
    - text selection: `Fill` rects from `layout.selection_rects`
    - caret: a `Caret` from `layout.caret`
    - preedit: an underline as an `Outline`
    - all transformed by the node's world transform and then the view
  - `"type" {text}` → `{ok}` when editing; otherwise `{ok:false, error:"not editing text"}`.

- [ ] **Step 1: Write the failing tests**
  - `sawit-board`:
    - `typing_into_a_new_sticky`: `board.sticky`, `textedit::begin`, `insert("hi")`, `end` → the sticky's text is `"hi"`.
    - `ime_commit_replaces_preedit`: `preedit("ni")`, then `insert_ime_commit("你")` → text `"你"`.
    - `cancelled_composition_leaves_no_text`: `preedit("ni")`, then `route_key("Escape")` → text `""` and editing ended.
    - `caret_overlay_while_editing`: while editing, `overlay(..)` contains one `Caret`.
    - `type_op_types_text`.
  - App (`#[gpui_kit::test]`): `input_handler_inserts_text`. On a `BoardView` editing a new sticky, call `replace_text_in_range(None, "ok", ..)` through `update`, then the sticky reads `"ok"`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board && cargo test -p sawit board::`
Expected: FAIL.

- [ ] **Step 3: Implement** `text_input.rs`, the overlay additions, the preedit render path and the op.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board && cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/board crates/board/src
git commit -m "feat(canvas): type and compose text on the native canvas"
```

### Task 6: Clipboard across tabs and with the system

**Files:**
- Create: `crates/board/src/clipboard.rs`
- Modify: `crates/board/src/input.rs` (`edit.copy`/`edit.cut`/`edit.paste*` routed through `clipboard`)
- Modify: `crates/app/src/board/mod.rs` (system clipboard read and write)

**Interfaces:**
- Produces:
  ```rust
  pub fn after_copy(board: &BoardDoc) -> String;   // stores session.clipboard in a process-global slot; returns the marker "sawit-canvas:<generation>"
  pub fn before_paste(board: &mut BoardDoc);       // installs the global slot into this session's clipboard
  pub enum PasteSource { Canvas, Text(String), Image(Vec<u8>), Svg(String) }
  pub fn paste_source(system_text: Option<&str>, system_png: Option<&[u8]>, marker: Option<&str>) -> PasteSource;
  pub fn paste(board: &mut BoardDoc, src: PasteSource, at: Option<(f64, f64)>) -> Result<Vec<NodeId>, BoardError>;
  ```
- `paste_source` decides in this order:
  1. An image → `Image`.
  2. Text equal to the marker → `Canvas`.
  3. Text starting with `<svg` → `Svg`.
  4. Other non-empty text → `Text`.
  5. Otherwise → `Canvas`.
- `paste` does the following per source:
  - `Canvas` → `edit.paste`
  - `Text` → `node.create {type:"text", text, x, y}` at `at` or the viewport centre
  - `Image` / `Svg` → `file.import {data: base64, name: "pasted.png" | "pasted.svg"}`
- The app writes the marker with `cx.write_to_clipboard(ClipboardItem::new_string(marker))` after
  copy/cut. Before a paste it reads `cx.read_from_clipboard()`.

- [ ] **Step 1: Write the failing tests**
  - `copy_in_one_tab_pastes_in_another`: two `BoardDoc`s; copy a sticky in A, `before_paste` + `paste(Canvas)` in B → B has 1 sticky.
  - `paste_prefers_newer_system_text`: `paste_source(Some("hello"), None, Some("sawit-canvas:3"))` → `Text("hello")`.
  - `marker_means_canvas`: `paste_source(Some("sawit-canvas:3"), None, Some("sawit-canvas:3"))` → `Canvas`.
  - `svg_text_imports`: pasting `Svg` of a one-rect SVG adds 1 layer.
  - `empty_canvas_clipboard_is_a_quiet_error`: `paste(Canvas)` with nothing copied → `Err(BoardError::Engine(_))`. The app maps this to no toast.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board clipboard::`
Expected: FAIL.

- [ ] **Step 3: Implement** `clipboard.rs` and the app glue.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src crates/app/src/board
git commit -m "feat(canvas): copy and paste between canvases and from other apps"
```

### Task 7: Live verification

- [ ] **Step 1:** Launch as in Plan 1 Task 7 with an empty project (no `canvas.figcraft`).
- [ ] **Step 2: Drive the canvas over the socket.**
  - `{"cmd":"canvas","op":{"op":"command","command":"tool.sticky"}}`
  - `pointer` down and up at `(0,0)`
  - `{"op":"type","text":"Hello from Sawit"}`
  - `{"op":"key","key":"Escape"}`
  - Expected: `get` shows 1 element.
  - Screenshot: a sticky reading "Hello from Sawit" with no caret.
- [ ] **Step 3: Draw a selection.**
  - Send `{"op":"command","command":"tool.shape"}` and draw with `pointer` down → move → up.
  - Expected: the screenshot shows the selection box, 4 handles and the size label.
- [ ] **Step 4: Ask the user to check by hand** (synthetic clicks don't reach the window):
  - drag a shape
  - ⌘Z
  - type with a Chinese IME
  - ⌘B while editing versus not editing
  - ⌘W closes the tab
  - Record their answers in this plan.
- [ ] **Step 5: Stop the instance** with `/bin/kill`, then run the full gates:
Run: `cargo test --workspace`
Expected: all pass.
