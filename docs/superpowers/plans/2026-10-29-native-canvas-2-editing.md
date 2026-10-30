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

#### Task 7 notes: live check (2026-10-30, at `c16f0e3` and `17a6e48`, lane 1)

Steps 1–3 passed over the control socket, and so did the overlay checks carried over from Task
3's review. The check found one defect: the caret didn't show at the start of a line, so a new
sticky showed none. It is fixed in `17a6e48` (see "The caret at the start of a line" below).
`cargo test --workspace` passes: 1095 passed, 9 ignored and 0 failed at `c16f0e3`, and 1097
passed, 9 ignored and 0 failed at `17a6e48`. Step 4 needs the user: its checks are under "Hand
checks (pending — needs the user)" below, together with the ones carried over from the reviews of
Tasks 1–6. Machine: Apple M3 (8 cores, 16 GB), macOS 26.6.1, window scale factor 2, debug build.
Other lanes were compiling, so `vm.loadavg` was 2.9–10.8.

**How it ran.** The app was launched with Plan 1 Task 7's launch recipe for an empty board (no
`.nyawit/`), from lane 1. The debug build's only linker line is the known R-EHFRAME warning.
`{"cmd":"layout","terminal":false}` gave the canvas 1040×711 pt. The view stayed at zoom 1, with
page point (0, 0) in the middle of the canvas. Pointer ops take page points. The app ran four
times, each stopped with `/bin/kill $PID`: for Steps 1–3 and most checks; after a relaunch, for
the equal gaps; and for the caret, before and after its fix.

**Answers (Steps 1–3, and the checks carried over from Task 3).**

- Step 1: `open_page canvas` answers `{"tab":0}`, and `get` answers `[]`. Nothing is written until
  the first edit.
- Step 2:
  - `tool.sticky` answers `{"tool":"sticky"}`.
  - `pointer` down and up at (0, 0) place a sticky centred there and start typing in it. Both answer
    `selection: ["3"]` (the sticky's text) and `tool: "move"`.
  - `type "Hello from Sawit"` answers `{ok: true}`. A screenshot taken before Escape shows the text
    on two lines ("Hello from" / "Sawit") in the outlined text box, with the caret after "Sawit".
  - `key Escape` answers `handled: true`. `get` then shows 1 element: `"2"`, a sticky at
    (−100, −100), 200×200, `#FFE78A`, reading "Hello from Sawit".
  - The screenshot shows the sticky reading "Hello from Sawit" with no caret. The sticky itself is
    selected: outline, 4 handles and `200 × 200`.
  - The first edit created `.nyawit/canvas.figcraft` and `.nyawit/.gitignore`. The file's
    `document.json` holds the text.
- Step 3, read as "place a shape" (C19):
  - `tool.shape`, then `pointer` down at (200, −50), two moves to (320, 40), and up there.
  - The press places a 160×160 shape centred on it: `"4"`, at (120, −130). It also puts the Move
    tool back. The moves don't resize the shape.
  - The screenshot shows the shape's outline, 4 handles and `160 × 160`.
- Carried over from Task 3's review:
  - (a) `node.create` a frame (at (−470, −280), 260×150: `"7"`) and a section (at (−470, 20),
    300×220: `"8"`), then `select.none`. The screenshot shows "Frame 1" in grey just above the frame's
    top-left corner, and "Section 1" on a grey pill inside the section's top-left corner.
  - (b) `view.zoom100` answers `{"zoom":1.0}`. `pointer` down and up at (frame x + 4, frame y − 11),
    which is (−466, −291), select the frame (`selection: ["7"]`). The screenshot shows its outline,
    4 handles, `260 × 150`, and its name in blue.
  - (c) `tool.move`, then `pointer` down on empty canvas at (120, 150), and moves to (240, 230) with
    no up. The screenshot shows the 120×80 marquee: pale blue, with a blue outline. After the up it
    is gone, and nothing is selected.
- Also checked over the socket:
  - **A drag snaps, and is one undo step.**
    - Shape `"4"` was dragged by its middle: down at (200, −50), then moves to (206, −21).
    - It snapped level with the sticky (its top went from −101 to −100), and the screenshot shows a
      red-orange guide along both tops. After the up it is at (126, −100).
    - The key op ⌘Z (`{"key":"z","cmd":true}`) puts it back at (120, −130) in one step. ⇧⌘Z moves
      it again.
  - **Equal gaps get distance pills.**
    - A third shape was placed at x 320, in line with the sticky and shape `"4"`, then dragged 7 px
      left.
    - It snapped to x 312. There, its gap to `"4"` is the same as the gap between the sticky and
      `"4"`.
    - Each of the two gaps showed a red-orange line with a `26` pill.
  - **A double-click edits a sticky's text.**
    - `pointer` down, up, `double` and up on the sticky's text start the edit with all of the text
      selected. The highlight covers both lines, and the caret is at the end.
    - `type "x"` replaced all of the text.
    - The key op ⌘Z then ended the edit, and undid the typing in one step. The sticky reads "Hello
      from Sawit" again, and `type` answers "not editing text".
  - **Several layers selected.** A click on the sticky, then a shift-click on the shape, select both.
    Each is outlined, and a box around both has the 4 handles and `386 × 200` (C15).
  - **Hover.** A `hover` over the unselected shape with the Move tool outlines it in blue, without
    handles.
  - **After a restart.** After `/bin/kill`, a new launch on the same project opened the board as it
    was left.
  - **Idle while typing.** A sticky's text was being edited, with its caret showing. `top`'s 1 s
    samples of the app read 0.0–1.8 % CPU over 10 s, and no render line was logged. The caret that
    holds steady (Task 5, Decision 6) costs nothing at rest.
  - **The log.** All 30 lines of the four runs' debug logs are `canvas render` lines (90–157 ms).
    There are no warnings or errors.
- **The caret at the start of a line (a defect, fixed in `17a6e48`).**
  - In Step 2, the screenshot of the new sticky before typing showed its outlined text box, but no
    caret. With "Hello from Sawit" typed, a double-click and the key op ArrowLeft put the caret
    before the H: again no caret showed. After ArrowRight it showed, after the H.
  - The cause: the caret was a 1 px line in the selection colour, drawn on the line before its
    character. At the start of a line, that line is the left edge of the text's box, and the box's
    outline is the same colour and width there. FigCraft draws its caret in its theme's text colour,
    which the canvas's fixed colours (D4) leave out.
  - The fix: the caret is painted as a 2 px band from that line toward the text
    (`overlay::caret_band`), as a text view draws its insertion point. At a line's start, it stands
    inside the box, beside the outline. Tests: `a_caret_at_a_line_start_shows_inside_the_outlined_box`
    and `a_caret_band_runs_toward_the_text`.
  - After a rebuild, the same steps showed the caret: a thick bar inside the left edge, both before
    the H and in a new sticky. After the H, it is a 2 px bar between the H and the e.
- **Bold doesn't show in stickies on this Mac.**
  - ⌘B through the key op, with the sticky's text selected, answered `handled: true`. `node.get` on
    the text then gave `fontStyle: "Bold"`. But the screenshot showed no change.
  - The cause:
    - Stickies are set in Inter. This Mac has no Inter (`text.fonts {family: "Inter"}` answers
      `[]`), and Sawit embeds none.
    - FigCraft's own builds embed Inter through the craft-fonts build input (`CRAFT_FONTS_DIR`, set
      in its `.cargo/config.toml`).
    - figcraft-text lays out a missing family in its default face, Helvetica Neue Regular, whatever
      the style asked for (figcraft-text `layout.rs:356`, `fontdb.rs` `default_face`).
  - The render path is fine: a Helvetica Neue text layer bolded with `text.bold` showed bold.
  - So until Sawit embeds Inter, or Inter is installed, bold, italic and weight changes to Inter text
    change the document but not the canvas. This went to the controller as a concern. Hand check 4
    below works around it.

#### Hand checks (pending — needs the user)

Launch with Plan 1 Task 7's launch recipe for an empty board (skip the line that copies the example),
and run its `open_page` line. Click the canvas once, so that it has the keyboard. Then press R and drag
out three rectangles in a row, a little apart. Where a check already passed over the socket, it says
so.

1. **Drag a shape** (Step 4, and Task 3's review).
   - Press V. Drag a rectangle by its middle, and let go. It follows the pointer all the way, then
     stays there, selected: a blue outline, 4 corner handles and its size under it.
   - Drag it level with another rectangle. While the button is held, red-orange guides join the edges
     that line up, and the rectangle snaps into line.
   - Drag an end rectangle until the gaps on both sides of the middle one are equal. Each gap shows a
     red-orange line with a pill that gives its width in px.
   - The guides go when the button comes up.
   - Over the socket, the snap, the guide and the `26` pills showed as described above.
2. **A drag released outside the window** (Review Focus 1, and Task 2's review).
   - Press R. Press on the canvas, drag out past the window's edge, and release outside the window.
   - Expect one rectangle, selected, with the Move tool back. Moving back over the canvas draws
     nothing.
   - One ⌘Z removes the whole rectangle.
3. **⌘Z** (Step 4).
   - Right after a drag, ⌘Z puts the rectangle back where it was, in one step. ⇧⌘Z moves it again.
   - Double-click a sticky that has text, and type x: the x replaces the text. Press ⌘Z while still
     typing. Expect the edit to end, and the sticky to read its old text again, in one step.
   - Both passed through the key op.
4. **⌘B while editing versus not** (Step 4).
   - Click empty canvas, so that no text is edited, and press ⌘B. Sawit's left sidebar hides, and the
     canvas doesn't change. ⌘B again shows the sidebar.
   - Double-click a sticky, which selects all of its text, and press ⌘B. The sidebar stays where it
     is.
   - The word in a sticky doesn't look bold unless Inter is installed (see "Bold doesn't show in
     stickies on this Mac" above). To see the bold, make a text in an installed font:
     `ctl '{"cmd":"canvas","op":{"op":"command","command":"text.create","params":{"text":"Bold me","x":0,"y":300,"props":{"fontFamily":"Helvetica Neue","fontSize":24}}}}'`
     Double-click it, which selects all of its text, then press ⌘B. The text turns bold, and the
     sidebar stays.
5. **⌘W closes the tab** (Step 4).
   - Click the canvas, and press ⌘W. The Canvas tab closes.
   - Reopen it with ⌃⌘C, or the title bar's Canvas button. The board is as it was.
   - Double-click a sticky, type a word, and press ⌘W while still typing. The tab closes here too:
     ⌘W is Sawit's even while text is edited. Reopen it. The sticky reads what was typed.
6. **Cursors** (Task 2's review; P2-C3 shows only by eye).
   - R: a crosshair.
   - T: a crosshair over empty canvas, and an I-beam over a text layer. To make one: T, a click, a
     word, then Escape.
   - V, with a rectangle selected:
     - ↔ on its left and right edges, and ↕ on its top and bottom edges;
     - the ↖↘ diagonal on its top-left and bottom-right corners, and the ↗↙ diagonal on its top-right
       and bottom-left corners;
     - a crosshair just outside a corner (rotate).
   - H: an open hand, and a closed hand while the button is held.
7. **Double-click into a sticky** (Task 2's review).
   - Press Q, and click to place a sticky. Type a, press Escape, then V.
   - Double-click the sticky. Its text shows selected, and typing x replaces it. Without the edit, x
     would pick the Connector tool.
   - Over the socket, the double-click selected all of the text, and x replaced it.
8. **Panning by hand** (the reviews of Tasks 2 and 4).
   - Hold Space over the canvas: an open hand. Drag: the canvas follows the pointer, under a closed
     hand. Release Space: the tool's cursor is back.
   - With a mouse, press R, then drag with the middle button. The canvas pans, and nothing is drawn.
   - Drag with the middle button again, on past the window's edge, and release outside the window.
     The pan ends there: back over the canvas, the crosshair shows, not the closed hand.
   - While typing in a sticky, Space types a space.
9. **Shifted digits on a real keyboard** (Task 1's review; a US layout).
   - Select a shape, and zoom out. ⇧2 zooms to the selection, ⇧1 zooms to fit, and ⇧0 sets 100 %.
   - While typing in a sticky, ⇧⌘8 makes the paragraph a bulleted list, and ⇧⌘8 again ends it. ⇧⌘7
     makes a numbered list.
10. **Typing with a Chinese input method** (Step 4, Task 5's review, and Review Focus 3).
    - Set-up: add Pinyin – Simplified (System Settings › Keyboard › Text Input › Edit › +). Press Q,
      and click to place a sticky. You are now typing in it.
    - (1) With Pinyin, type ni.
      - ni shows underlined in the sticky. The candidate window opens just below it, from its left
        end: not at a corner of the window or of the screen. Check this again after a pinch zoom-in.
      - Press Space (or 1). The first candidate (你) replaces the underlined ni, and no space is
        typed.
      - Type ni, then press Return. Pinyin commits the letters ni, and no new line starts.
    - (2) Type ni, then press Escape. The letters vanish, and nothing goes in. The caret stays, and
      typing composes again. A second Escape ends the edit.
    - (3) Click away while composing:
      - Type ni, and click empty canvas. The edit ends with no underline, caret or candidate window
        left.
      - Write down what the sticky then reads: ni, 你, or nothing added. Each passes Review Focus 3.
        For comparison, write down what TextEdit keeps on the same click.
      - Double-click the sticky, and type a. The a composes alone, never nia.
    - (4) An edit that ends under the input method:
      - In another app's terminal (not Sawit's own), with the launch recipe's `ctl` and `SOCK`, run
        `sleep 5; ctl '{"cmd":"canvas","op":{"op":"key","key":"Escape"}}'`.
      - Switch back with ⌘Tab (a click would end the edit), and type ni in the sticky within the 5 s.
        The edit ends, and the underline goes.
      - Double-click the sticky, and type a. Write down whether the a composes alone, or nia shows.
    - (5) Switch to ABC or U.S., with press-and-hold on (the macOS default). Type caf, then hold e.
      The accents menu opens by the e. Choose é: the sticky reads café.
    - (6) ⌥E: ´ shows underlined at the caret. Then E: é, with no ´ left.
    - (7) Space: while typing in a sticky, Space types a space (check 8). With Pinyin on and nothing
      composed, Space types a space too.
    - (8) The caret holds steady, where FigCraft's blinks (Task 5, Decision 6). Say whether that is
      fine. Live, it cost 0–1.8 % CPU at rest. It is a 2 px blue bar since `17a6e48`, so that it
      shows at the start of a line too: check that it shows in a new sticky, and after ⌘← (the
      start of the line).
    - (9) While ni is composing, pan with two fingers. The candidate window stays where it was until
      the next key (Task 5's review, a deferred minor). Say whether that bothers you.
11. **Pasting while composing** (Task 6's review).
    - Copy hello in TextEdit. In a sticky being edited, with Pinyin, type ni: it is underlined, and
      the candidates are open. Then press ⌘V.
    - hello is in the sticky. No underlined ni stays once nothing is being composed. Typing on
      composes and commits as usual.
    - Write down where the composition's own text went, if anywhere.
12. **The system clipboard** (Task 6's review). The socket can't reach it: the key op answers
    `handled: true` for ⌘C without copying (C7).
    - (a) Select a sticky, ⌘C, then ⌘V in TextEdit. It types `sawit-canvas:N`, where N counts the
      copies since launch.
    - (b) Copy a word in TextEdit, click empty canvas, and press ⌘V. A text layer reads that word, its
      top-left in the middle of the view, and no toast shows.
    - (c) Press ⌃⇧⌘4, and drag over part of the screen (the capture goes to the clipboard). Then ⌘V
      on the canvas: an image layer shows it.
    - (d) In a text editor, copy the source of an SVG file that starts with `<?xml`. Then ⌘V on the
      canvas: its shapes come in, in a frame, not as a text layer.
    - (e) ⌘C a sticky in one canvas, and ⌘V in another: the canvas of a second Sawit window on
      another folder, or a second canvas tab once Plan 7's `.figcraft` file tabs are in. The sticky
      pastes there, and ⇧⌘V pastes it again.
    - (f) While typing in a sticky, select a word and press ⌘C. Then ⌘V in TextEdit types that word,
      not a marker.
    - (g) Copy text in TextEdit, then press ⌘V in a sticky being edited. The text is typed at the
      caret, and no layer appears.
    - (h) ⌘C a sticky, quit Sawit with ⌘Q, and relaunch it. ⌘V on the canvas pastes nothing: no
      toast, and no `sawit-canvas:N` text layer (Decision 15).
13. Plan 1's hand check 5 ends with a check that waited for Plan 2's editing, and it can run now: move
    a layer with the mouse, press ⌘Q at once, relaunch, and open the canvas. Expect the move to be
    kept.

Write the answers here once the checks have run (R5).
