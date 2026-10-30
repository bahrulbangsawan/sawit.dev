# Native canvas 5: Design panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A right Design panel (264 px) with nine sections:
1. Header
2. Position and size (with align and distribute)
3. Auto layout
4. Appearance
5. Fill
6. Stroke
7. Effects
8. Typography
9. Export

All of them handle mixed values across the selection, and scrubbing a number field is one undo step.

**Architecture:** One key space for reading and writing:
- **Reads** come from `figcraft_engine::inspect::node_json(doc, id, 0)`. It uses the same property keys
  as `node.set`, and fills in defaults for keys it omits.
- **Writes** are `node.set {ids, props}`. The key list is `figcraft_engine::cmd::props::KEYS`.

`sawit-board/src/design/` turns the selection into section models: `Mixed` values, paint and effect
rows, input parsing, and edits as `(command, params)`. The app's `design_panel/` renders them with
gpui-kit inputs and the colour picker.

**Tech Stack:** As Plan 1. gpui-kit `Input`, `NumberInput`, `Select`, `ColorPicker` (`ColorPickerState`), `Switch`, `Button`.

**Spec:** `docs/superpowers/specs/2026-10-29-native-canvas-design.md` ("Design panel")

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

1. An edit to a mixed field must apply to every selected layer, and stay one undo step. Test: `editing_mixed_x_moves_all_in_one_step` (Task 1).
2. Invalid input (`abc`, `-5` for width, `1e400`, an empty string) must be rejected without changing the document. Test: `parse_rejects_bad_numbers` (Task 1).
3. Scrubbing a field must produce one undo entry however many previews ran. Escape during a scrub must restore the start value. Tests: `scrub_is_one_undo_step`, `cancelled_scrub_restores` (Task 4).
4. Gradient and image paints must never be silently rewritten as solid. Only an explicit "replace with solid" changes them. Test: `editing_one_solid_keeps_gradient_neighbour` (Task 2).
5. A text layer with mixed runs (two font sizes) must show Mixed, and setting a size must apply it to the whole layer. Test: `mixed_runs_show_mixed_size` (Task 3).

---

### Task 1: Selection model, `Mixed`, number parsing, position and appearance

**Files:**
- Create: `crates/board/src/design/mod.rs`
- Create: `crates/board/src/design/fields.rs`
- Modify: `crates/board/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Mixed { Absent, Same(serde_json::Value), Mixed }
  pub enum Section { Header, Position, AutoLayout, Appearance, Fill, Stroke, Effects, Typography, Export }
  pub struct Edit { pub command: &'static str, pub params: serde_json::Value }
  pub struct DesignSelection { pub ids: Vec<NodeId>, /* private: per-node prop maps */ }
  impl DesignSelection {
      pub fn read(doc: &Document, ids: &[NodeId]) -> Self;          // inspect::node_json + DEFAULTS
      pub fn get(&self, key: &str) -> Mixed;
      pub fn sections(&self) -> Vec<Section>;
      pub fn header(&self) -> String;                               // "Rectangle", or "3 layers"
      pub fn set(&self, key: &str, value: serde_json::Value) -> Edit; // node.set {ids, props:{key: value}}
  }
  pub const DEFAULTS: &[(&str, &str)];   // key → JSON default: visible true, locked false, opacity 1, rotation 0, cornerRadius 0, blend "normal"
  pub fn parse_number(key: &str, input: &str) -> Option<f64>;
  pub fn align(command: &'static str) -> Edit;   // align.left|hcenter|right|top|vcenter|bottom, distribute.horizontal|vertical
  ```
- `parse_number`:
  - trims the input and accepts `50%` for `opacity` (→ 0.5)
  - rejects non-finite values
  - rejects values < 0 for `width`, `height`, `strokeWeight`, `fontSize`, `gap`, `padding` and
    `cornerRadius`
  - clamps `opacity` to 0..=1
- `sections` rules:
  - `Position`, `Appearance`, `Effects` and `Export`: always shown.
  - `AutoLayout`: when every selected layer is frame-like.
  - `Fill` and `Stroke`: unless only groups are selected.
  - `Typography`: when any selected layer is text.

- [ ] **Step 1: Write the failing tests** (design doc with two rectangles R1 (x 0) and R2 (x 50), and one text):
  - `same_values_read_same`: the width of both 100×100 → `Same(100)`.
  - `different_values_read_mixed`: `x` → `Mixed`.
  - `defaults_fill_omitted_keys`: `visible` → `Same(true)` and `opacity` → `Same(1)`.
  - `editing_mixed_x_moves_all_in_one_step`: execute `set("x", 10)` → both x are 10; one `edit.undo` → back to 0 and 50.
  - `parse_rejects_bad_numbers`: `parse_number("width","abc")`, `("width","-5")`, `("x","1e400")` and `("x","")` → `None`; `("x","-5") == Some(-5.0)`; `("opacity","50%") == Some(0.5)`; `("opacity","3") == Some(1.0)`.
  - `sections_by_kind`: rectangles → no `Typography` and no `AutoLayout`; text → has `Typography`; a frame → has `AutoLayout`.
  - `header_names_kind_or_count`: `"Rectangle"`; `"2 layers"`.
  - `align_left_runs`: `align("align.left")` executed → both x equal.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board design::`
Expected: FAIL.

- [ ] **Step 3: Implement `design/mod.rs` and `fields.rs`.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board design::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/design crates/board/src/lib.rs
git commit -m "feat(canvas): design panel model with mixed values and safe number input"
```

### Task 2: Paints and effects

**Files:**
- Create: `crates/board/src/design/paints.rs`
- Create: `crates/board/src/design/effects.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum PaintKind { Solid { hex: String, opacity: f64 }, Gradient(String), Image, Other }
  pub struct PaintRow { pub kind: PaintKind, pub visible: bool }
  pub fn paints(sel: &DesignSelection, key: &str) -> Option<Vec<PaintRow>>;   // key "fills" | "strokes"; None when mixed
  pub fn set_solid(sel: &DesignSelection, key: &str, index: usize, hex: &str, opacity: f64) -> Option<Edit>; // None for a bad hex
  pub fn add_paint(sel: &DesignSelection, key: &str) -> Edit;                  // appends solid #D9D9D9, or #000000 for strokes
  pub fn remove_paint(sel: &DesignSelection, key: &str, index: usize) -> Edit;
  pub fn replace_with_solid(sel: &DesignSelection, key: &str, index: usize) -> Edit;
  pub fn stroke_fields() -> &'static [&'static str];                           // ["strokeWeight", "strokeAlign", "dashes"]
  pub enum EffectKind { DropShadow, InnerShadow, LayerBlur, BackgroundBlur }
  pub struct EffectRow { pub kind: EffectKind, pub visible: bool, pub x: f64, pub y: f64, pub blur: f64, pub spread: f64, pub hex: String, pub opacity: f64 }
  pub fn effects(sel: &DesignSelection) -> Option<Vec<EffectRow>>;
  pub fn add_effect(sel: &DesignSelection, kind: EffectKind) -> Edit;
  pub fn set_effect(sel: &DesignSelection, index: usize, row: &EffectRow) -> Edit;
  pub fn remove_effect(sel: &DesignSelection, index: usize) -> Edit;
  ```
- Paint and effect JSON go out exactly in the shapes `node.set` accepts. Read the `"fills"`, `"strokes"`
  and `"effects"` handlers in `crates/engine/src/cmd/props.rs`. Rows not being edited are re-sent
  unchanged (the JSON value as read).

- [ ] **Step 1: Write the failing tests:**
  - `solid_fill_reads_hex_and_opacity`.
  - `set_solid_changes_one_row`: two fills → only `index 1` changes.
  - `editing_one_solid_keeps_gradient_neighbour`: a linear gradient fill plus a solid; editing the solid → the gradient is identical afterwards.
  - `bad_hex_is_rejected`: `"#12"` → `None`.
  - `add_and_remove_paint`.
  - `drop_shadow_round_trip`: add a DropShadow, set x 4, y 4, blur 8, `#000000` 25 % → reading back gives the same row.
  - `mixed_paints_are_none`: two rectangles with different fills → `paints(..)` is `None`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board design::`
Expected: FAIL.

- [ ] **Step 3: Implement** `paints.rs` and `effects.rs`.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board design::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/design
git commit -m "feat(canvas): fill, stroke and effect rows for the design panel"
```

### Task 3: Auto layout, typography and export models

**Files:**
- Create: `crates/board/src/design/layout.rs`
- Create: `crates/board/src/design/typography.rs`
- Create: `crates/board/src/design/export.rs`

**Interfaces:**
- Produces:
  ```rust
  // layout.rs: keys layout, gap, padding, paddingTop/Right/Bottom/Left, mainAlign, crossAlign, wrap, sizingH, sizingV
  pub fn add_layout(sel: &DesignSelection) -> Edit;      // "layout.add"
  pub fn remove_layout(sel: &DesignSelection) -> Edit;   // "layout.remove"
  pub fn align_cell(sel: &DesignSelection, row: u8, col: u8) -> Edit; // 3×3 grid → mainAlign/crossAlign min|center|max per layout direction
  // typography.rs: keys fontFamily, fontStyle, fontSize, lineHeight, letterSpacing, textAlign, textAlignVertical, textAutoResize
  pub fn families() -> Vec<String>;                      // figcraft_text::family_names(), sorted, deduped
  pub fn styles(family: &str) -> Vec<String>;            // the styles figcraft_text reports for the family; else ["Regular","Bold"]
  // export.rs
  pub struct ExportRow { pub format: String, pub scale: f64 }
  pub fn export_rows(sel: &DesignSelection) -> Vec<ExportRow>;              // from "exports"; empty → one PNG 1×
  pub fn set_export_rows(sel: &DesignSelection, rows: &[ExportRow]) -> Edit;
  pub fn export_bytes(board: &mut BoardDoc, ids: &[NodeId], row: &ExportRow) -> Result<Vec<u8>, BoardError>; // "file.export" {ids, format, scale} → base64 → bytes
  ```

- [ ] **Step 1: Write the failing tests:**
  - `add_layout_then_gap`: a frame with 2 children → `add_layout`, then `set("gap", 12)` → `get("gap") == Same(12)`.
  - `align_cell_maps_to_main_and_cross`: horizontal layout, cell (row 1, col 2) → `mainAlign "max"` and `crossAlign "center"`.
  - `mixed_runs_show_mixed_size`: text "Hello world", then `text.style {id, start: 0, end: 5, fontSize: 24}` → `get("fontSize") == Mixed`; `set("fontSize", 18)` → `Same(18)`. If `inspect` reports runs separately, `DesignSelection::read` folds them into `Mixed`.
  - `families_are_sorted_and_unique`.
  - `export_defaults_to_png_1x`.
  - `export_bytes_png_has_signature`: the bytes start with `\x89PNG`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board design::`
Expected: FAIL.

- [ ] **Step 3: Implement** the three modules.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board design::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/design
git commit -m "feat(canvas): auto layout, typography and export models"
```

### Task 4: Panel shell, Position, Appearance, Auto layout, scrubbing

**Files:**
- Create: `crates/app/src/board/design_panel/mod.rs` (shell, section order, `show_design` default: false on boards, true on design files)
- Create: `crates/app/src/board/design_panel/number_field.rs`
- Create: `crates/app/src/board/design_panel/position.rs`
- Create: `crates/app/src/board/design_panel/appearance.rs`
- Create: `crates/app/src/board/design_panel/layout.rs`
- Modify: `crates/app/src/board/mod.rs` (the `ui_requests` kind `panel.design`, from `view.design`, toggles the panel; a top-bar toggle)

**Interfaces:**
- Consumes: `design::*`.
- Produces:
  ```rust
  pub struct NumberField { key: &'static str, label: SharedString }  // shows Mixed as "Mixed"
  impl BoardView {
      pub fn apply_edit(&mut self, edit: Edit, cx: &mut Context<Self>);            // execute; error → toast
      pub fn scrub_begin(&mut self, key: &'static str, cx: &mut Context<Self>);   // session.begin_interaction(key)
      pub fn scrub_to(&mut self, key: &'static str, value: f64, cx: &mut Context<Self>); // node.set inside the interaction
      pub fn scrub_end(&mut self, commit: bool, cx: &mut Context<Self>);          // commit_interaction | cancel_interaction
  }
  ```
- `NumberField`:
  - Its label is a drag handle: 1 unit per px, ×10 with ⇧.
  - The input commits on Enter or blur through `parse_number`.
  - On rejected input the field shows the current value again.
- Rows:
  - **Position:** X Y / W H / rotation, a constrain-proportions toggle, and 8 align/distribute buttons.
  - **Appearance:** opacity (%) and corner radius. "Individual corners" expands to 4 fields; blend mode
    is a `Select`.
  - **Auto layout:** add/remove, direction (H, V, wrap), gap, padding (uniform; "individual" expands to
    4), the 3×3 alignment grid, sizing for W and H (fixed, hug, fill).

- [ ] **Step 1: Write the failing tests** (`#[gpui_kit::test]`):
  - `scrub_is_one_undo_step`: `scrub_begin("x")`, `scrub_to` ×20, `scrub_end(true)`; one `edit.undo` → back to the start.
  - `cancelled_scrub_restores`: `scrub_end(false)` → x is the start value, and the undo stack depth is unchanged.
  - `apply_edit_error_sets_toast`: an `Edit` for an unknown command → `toast.is_some()`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit board::design_panel`
Expected: FAIL.

- [ ] **Step 3: Implement** the shell, `NumberField` and the three sections.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/board
git commit -m "feat(canvas): design panel with position, appearance and auto layout"
```

### Task 5: Fill, Stroke, Effects, Typography, Export views

**Files:**
- Create: `crates/app/src/board/design_panel/paints.rs` (shared by Fill and Stroke)
- Create: `crates/app/src/board/design_panel/effects.rs`
- Create: `crates/app/src/board/design_panel/typography.rs`
- Create: `crates/app/src/board/design_panel/export.rs`

**Interfaces:**
- Consumes: `design::{paints, effects, typography, export}`, `ColorPickerState`.
- Produces:
  - **Paint row:**
    - Swatch button → `ColorPicker` popover. A change sends `set_solid`; while the picker is open the
      changes are a scrub (one undo step).
    - Hex input, opacity %, eye toggle, remove.
    - Gradient and image rows show a label and offer "Replace with solid".
    - Mixed paints show "Mixed" with a "Replace all" button.
  - **Stroke:** extra rows for weight, align (`Select`) and dashed (`Switch` → `dashes` `[4,4]` / `[]`).
  - **Effects:** kind `Select`, eye toggle, an expandable editor for x/y/blur/spread/colour (radius only
    for blurs), and remove.
  - **Typography:**
    - Family: a searchable `Select` over `families()`.
    - Style: a `Select` over `styles(family)`.
    - Size, line height and letter spacing fields.
    - Align H/V buttons and auto-resize buttons.
  - **Export:**
    - Rows of format `Select` (PNG, JPG, SVG, PDF) and scale (0.5×, 1×, 2×, 3×, 4×).
    - An "Export N layers" button that writes `export_bytes` to
      `nyawit::unique(nyawit::exports_dir(project), "<name>.<ext>")`, or the app root's `exports/`
      without a project, then runs `open -R`.

- [ ] **Step 1: Write the failing tests** (`#[gpui_kit::test]`):
  - `color_pick_is_one_undo_step`: open the picker (scrub begin), set 3 colours, close (commit) → one undo.
  - `export_writes_into_nyawit_exports`: a project canvas, a rectangle selected, `BoardView::export_selection(cx)` → a `.png` exists in `<project>/.nyawit/exports/`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit board::design_panel`
Expected: FAIL.

- [ ] **Step 3: Implement** the four views and `export_selection`.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/board
git commit -m "feat(canvas): fills, strokes, effects, typography and export in the design panel"
```

### Task 6: Live verification

- [ ] **Step 1:** Launch with `ridgeline-app.figcraft` as the canvas and select one card with `{"op":"command","command":"select.set","params":{"ids":[<id from get>]}}`.
  - Expected: the screenshot shows the Design panel with every section that applies.
- [ ] **Step 2:** Run `{"op":"command","command":"select.all"}`.
  - Expected: the screenshot shows "N layers" and "Mixed" fields.
- [ ] **Step 3: Ask the user to check by hand:**
  - scrubbing X
  - typing W
  - the colour picker
  - adding a drop shadow
  - font change
  - Export 2× PNG
  - Record their answers.
- [ ] **Step 4: Stop the instance** with `/bin/kill`, then run the full gates:
Run: `cargo test --workspace`
Expected: all pass.

#### Task 6 notes: live check (2026-10-30, at `b42ec77`, lane 5)

Steps 1 and 2 passed over the control socket, and so did every check that the delegates' rulings on
Tasks 4 and 5 carried to this task. The check found no defect that the ledger doesn't already hold
(see "Seen again, not new" below), so it has no fix commit. Step 3 needs the user: its checks are
under "Hand checks (pending — needs the user)" below, with the ones those rulings asked for.
`cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean
at `b42ec77`, and `cargo test --workspace` passes: 1222 passed, 9 ignored, 0 failed. Machine: Apple
M3 (8 cores, 16 GB), macOS 26.6.1, window scale factor 2, debug build. Other lanes were compiling,
so the timing is under build load (R-LOAD): `vm.loadavg` was 10.8 at the build, 11.2 at the launch
and 2.2 when the app stopped.

**How it ran.** As Plan 1 Task 7's launch recipe has it (R-LAUNCH), from lane 5, with a copy of
`ridgeline-app.figcraft` (SHA-1 `3726c59b…`) as the scratch project's `.nyawit/canvas.figcraft`.
An earlier attempt of this task died before it wrote anything down, so every check was run again,
on one instance (21:46–21:58) that was stopped with `/bin/kill`.
- After `open_page`, the canvas painted one frame before any other op (the P3.T4 ruling), and each
  batch of ops began with a `get`.
- Each batch of screenshots began with `{"cmd":"focus"}`, the one call that raises the window, and
  every shot waited for the window to be on screen with nothing over it.
- `{"cmd":"layout","terminal":false}` gave the canvas the terminal's room. A window can't be
  resized over the socket, so this instance's own frame was set through the accessibility API
  (position and size only: no mouse, key or accessibility action reached the app), from 1280 × 801
  pt to 1280 × 856, the screen's visible height. macOS keeps a window from growing taller, so what
  lies below the fold of a 856 pt window is in the hand checks.
- Only the check changed the board: the last `get` listed the file's 4 layers and the 12 that the
  check made, and the zoom read 25 % in every screenshot.
- The panel is 264 pt wide: its 1 pt border (`#363c46`) is at x = 1016 pt of the 1280 pt window, its
  background is `#282c33`, and the canvas's `#1e1e1e`.

**Answers (Steps 1–2).**

- Step 1, as P5-C25 amends it. `get` lists the 4 top-level layers: `"349"` Components, `"68"` Home —
  Light, `"239"` Home — Dark and `"178"` Trail — Light, and no card. `node.find {name: "Trail card"}`
  answers `{"ids":[29],"total":1}`, and `select.byName {query: "Trail card"}` answers
  `{"selection":[29]}`. The card is a component with a vertical auto layout: 353 × 256.3 at (60,
  800), radius 20, one white fill, two drop shadows, no stroke and no export setting.
  - The screenshot shows the header "Component  Trail card", then:
    - Position: X 60, Y 800, W 353, H 256.3, rotation 0, and the 8 align and distribute buttons in
      one row, the two distribute ones dimmed because one layer is selected.
    - Auto layout, with "−" in its header: the vertical button marked, the 3 × 3 grid with its
      top-left cell marked, gap 0, padding 0, W "Fixed" and H "Hug".
    - Appearance: opacity 100 %, radius 20, blend "Pass through".
    - Fill: FFFFFF 100 %. Stroke: no row. Effects: two "Drop shadow" rows.
    - Export: "PNG 1×", the row that a layer with no setting shows. Its "Export 1 layer" button is
      half under the window's bottom edge: the panel scrolls, and no op scrolls it (hand check 10).
  - Every field has its label or its icon, and nothing is clipped or overlapped.
- Step 2. `select.all` with the card selected answers `{"selection":[14,19,28,29,55]}`: the engine
  selects the card's siblings in its section. The screenshot shows the header "5 layers", then:
  - X, Y, W and H "Mixed", rotation 0, and all 8 align and distribute buttons on;
  - Auto layout with "+" and "−" (some of the five have one), no direction or grid cell marked, gap
    and padding "Mixed", W "Fixed" and H "Hug";
  - opacity 100 %, radius "Mixed", blend "Pass through";
  - Fill, Stroke and Effects "Mixed", each with "Replace all", and under Stroke: weight 1, "Inside"
    and Dashed off.
- After `select.none`, `select.all` takes the page's top-level layers: "16 layers" (the file's 4 and
  the check's 12), every box field "Mixed", and Typography showing the values of its one text layer
  (P5-C2).

**Also checked over the socket** (the delegates' carries, and the Review Focus).

- **A mixed selection** (the P5.T4 ruling). The file's frames have no auto layout, so the two layers
  are components that have: `node.set {ids: [29], props: {blend: "multiply"}}` on the card and
  `node.set {ids: [55], props: {sizingH: "hug"}}` on the Trail row (horizontal, fixed W, hug H),
  then `select.set {ids: [29, 55]}`.
  - The panel reads "2 layers": X "Mixed" (60 and 460), Y 800, W 353, H "Mixed"; no direction or
    grid cell marked, gap and padding "Mixed", the W menu "Mixed" and the H menu "Hug"; radius
    "Mixed" and the blend menu "Mixed"; Fill FFFFFF; Effects "Mixed" (2 and 1); and the button
    "Export 2 layers".
  - The 8 buttons are in one row, every field has its label, and nothing is clipped.
  - Two `edit.undo` put both layers back: `node.get` reads blend and sizing as before.
- **Auto layout and a plain frame** (the P5.T1 ruling). A horizontal frame (gap 8, padding 12) with
  Home — Light, which has none: direction and grid unmarked, gap and padding "Mixed", Fill "Mixed".
- **⌥8** (the P5.T4 ruling). `{"op":"key","key":"8","alt":true}` answers `handled: true` both
  times. The first press hides the Design panel and the top bar's toggle loses its mark. The second
  brings the panel back with the card's sections, and the toggle marked.
- **⌘\\** (P5-C18). `{"op":"key","key":"\\","cmd":true}` answers `handled: true`. The top bar, the
  toolbar and the Design panel go, and the canvas takes their room. The selection's outline and
  size stay: they are the canvas's own. ⌘\ again brings all three back, the panel with them.
- **A layer made to show every row** (the P5.T5 ruling). `node.create` made the rectangle `"350"`
  "P5T6 probe", 220 × 140 at (1500, 0): fills `#3B82F6` and a linear gradient (`#FF7A59` to
  `#6C5CE7`), a `#111827` stroke of weight 2, centred, dashes `[4,4]`, one drop shadow (black 40 %,
  offset 2 and 4, blur 8), exports PNG 2× and SVG, radius 12.
  - Header "Rectangle  P5T6 probe". Fill has two rows, the top paint first as in FigCraft's own
    panel: "Linear gradient", with its replace-with-solid button, eye and remove, then `3B82F6`
    100 %. Stroke: `111827` 100 %, "Weight 2", "Center", and the Dashed switch on. Effects: one "Drop
    shadow" row. Export: "PNG 2×" and "SVG 1×" (the scale of an SVG is dimmed: it makes no
    difference), and the whole "Export 1 layer" button.
  - Nothing is clipped or overlapped, at 856 pt, in a panel that needs no scrolling.
- **A text** (the P5.T5 ruling). `node.create` made the text `"351"` "P5T6 text": Helvetica Neue
  Medium 24, line height 150 %, letter spacing 2 % (a system family, as the P2.T7 ruling says: Inter
  isn't installed here, and draws in Helvetica Neue until Plan 8 embeds it).
  - Typography: the family menu "Helvetica Neue", style "Medium", size 24, line height 150 %, letter
    spacing 2 %, the 4 horizontal align buttons with Left marked and the 3 vertical ones with Top
    marked, and the 4 resize buttons with width and height marked. Fill, Stroke, Effects and Export
    show above and below it, whole.
  - `text.style {id: 351, start: 0, end: 4, fontSize: 40}` gave it two sizes (Review Focus 5): the
    size field reads "Mixed" while the node's own `fontSize` stays 24. `edit.undo` put it back.
- **The panel follows the document.** `node.set {ids: [350], props: {x: 1520, width: 260}}`: X 1520,
  W 260. `edit.undo`: 1500 and 220. `edit.redo`: 1520 and 260. `edit.undo`: 1500 and 220.
- **Every kind of layer.** One of each, selected in turn: ellipse, polygon, star, line, arrow,
  vector, frame (horizontal auto layout), slice, component, section, group, instance, and the
  rectangle and the text above. Each shows the sections that apply and nothing outside the 264 pt
  column. Auto layout shows for frames, components and instances ("+" while they have none);
  Typography only with a text; a group has no Fill or Stroke; a line and an arrow have a stroke
  row, `000000`, and no fill.
- **109 layers.** All the descendants of Home — Light: "109 layers", every box field "Mixed", the
  blend menu "Mixed", and Typography with the family "Inter" and its style, size, line height and
  letter spacing "Mixed". The panel had drawn by the shot, taken a second after the select.
- **A board** (D11). A second project, an empty board, in the same window (`open`, then
  `open_page`): with a blue sticky placed by `board.sticky` and selected, the board shows the
  context bar and no Design panel. `{"op":"key","key":"8","alt":true}` shows the panel, with the
  sticky's sections: "Frame  Sticky", Position, Auto layout (vertical, gap 8, padding 16, W
  "Fixed", H "Hug"), Appearance (radius 2), Fill `ACD3FA`, Effects (one drop shadow) and Export.
  The key again hides it.
- **The smallest window.** At 720 × 480 pt, with Sawit's sidebar shown: the panel keeps its 264 pt
  and its fields, the canvas keeps the rest (about 295 pt) and its toolbar scrolls inside it, and
  nothing overlaps.
- **Export, the engine's half** (Step 3). `file.export {ids: [350], format: "png", scale: 2}`, the
  call that the Export button makes, answers a 492 × 332 px PNG (13,618 bytes): the layer's
  440 × 280 px, and the shadow's and stroke's room round it.
- **The log** holds 32 `canvas render` lines and nothing else: no warning, no error. In ms, in the
  debug build under build load: min 56, p50 107, p95 154, max 526 (the first render).

**Seen again, not new.** The check left both as they are:
- A gradient row's swatch is the theme's background-to-foreground ramp, grey, whatever the stops
  (the probe's are orange to violet, the Photo layer's light blue). This is Task 5's deferred
  minor (paints.rs:207-216, ledger): a real swatch needs the stops in `PaintKind::Gradient`. Left
  for Plan 5's final review.
- A slice, which paints nothing, shows Appearance, Fill, Stroke and Effects, and a sticky's header
  reads "Frame" (the engine's kind: a sticky is a frame). The spec doesn't say which kinds show
  which sections, apart from Auto layout and Typography, so neither is a deviation.

#### Hand checks (pending — needs the user)

Launch with Plan 1 Task 7's launch recipe, with `ridgeline-app.figcraft` copied over
`.nyawit/canvas.figcraft` before the launch (as in Plan 3 Task 4's Step 3), and run its `open_page`
line. A design file opens with the Design panel shown. Click the canvas once, so that it has the
keyboard. These two lines make the layers the checks use, below the three phone frames (each layer
is selected when it is made; ⇧2 zooms to the selection):

```bash
ctl '{"cmd":"canvas","op":{"op":"command","command":"node.create","params":{"type":"rectangle","x":500,"y":900,"width":220,"height":140,"name":"P5T6 probe","props":{"fills":["#3B82F6",{"type":"linearGradient","stops":["#FF7A59","#6C5CE7"]}],"strokes":["#111827"],"strokeWeight":2,"strokeAlign":"center","dashes":[4,4],"effects":[{"type":"dropShadow","color":{"r":0,"g":0,"b":0,"a":0.4},"offset":[2,4],"radius":8,"spread":0}],"exports":[{"format":"png","size":{"by":"scale","value":2}},{"format":"svg","size":{"by":"scale","value":1}}],"cornerRadius":12}}}}'
ctl '{"cmd":"canvas","op":{"op":"command","command":"node.create","params":{"type":"text","text":"Typography probe","x":500,"y":1100,"name":"P5T6 text","props":{"fontFamily":"Helvetica Neue","fontStyle":"Medium","fontSize":24,"lineHeight":"150%","letterSpacing":"2%"}}}}'
```

The Trail card is the card at the lower left of the Components section (zoom in with ⌘+ to hit it).
Where a check already passed over the socket, it says so.

1. **Scrub X** (Step 3, the P5.T4 ruling, and Review Focus 3).
   - Select the probe rectangle. Drag the "X" label in the Position row sideways. Over the label
     the pointer is a left-right arrow. X moves 1 per px, and 10 per px with ⇧ held, and the
     rectangle follows on the canvas as you drag.
   - Let go with the pointer over the canvas, and again outside the window: the drag ends where it
     is, either way.
   - One ⌘Z puts X back where it was before the drag: the whole drag was one step. ⇧⌘Z does it
     again.
   - Drag again, and press Escape before letting go: X goes back to its start value and the drag is
     over. ⌘Z now undoes the step before this drag, not this one.
   - Drag the opacity field's label: opacity moves 1 % per px.
   - Select two layers whose X differ and drag the X label. The first pixel sets both to one X,
     counted from the first layer's, where Figma moves each layer from its own X. This is Task 4's
     deferred minor, recorded here for you: say whether it is acceptable.
2. **Type W** (Step 3, the P5.T4 ruling, and Review Focus 2).
   - Click in W: its text shows selected. Type 40 and press Enter: the rectangle is 40 wide, and the
     field reads 40. Click the rectangle on the canvas and press R: the Rectangle tool comes on, so
     the canvas has the keyboard again.
   - Type 60 in W and click in another field, with no Enter: 60 is applied when W lets go of the
     keyboard.
   - Type `abc`, `-5`, `1e400` and nothing at all, each followed by Enter. Each time the field shows
     the old value again, the rectangle doesn't change, and no toast shows.
   - Turn on the proportions lock (the link button beside H): typing 100 in W changes H in step.
   - Type 50, with no %, in the opacity field. It reads 50 %, and the rectangle fades.
   - Click in X and press R: an "r" appears in the field, and the tool doesn't change.
   - After Enter, the field keeps the keyboard with the caret at its end, so typing 5 next reads
     405. This is Task 4's deferred minor too: say whether Enter should select the text again, as a
     click into the field does.
3. **Mixed selections** (Review Focus 1, and P5-C2).
   - Select two layers whose X differ (click one, ⇧-click the other). X reads "Mixed". Type 100 in
     it and press Enter: both layers go to X 100, and one ⌘Z puts both back.
   - Select the text and the rectangle together. Typography shows, with the text's values. Set the
     size to 18: only the text changes, and no toast shows.
   - Select two layers with different fills. Fill reads "Mixed" with "Replace all". Click it: both
     get one new grey fill, and one ⌘Z restores both.
4. **Menus leave the canvas its keys** (the P5.T4 ruling on blend, and the P5.T5 one on the other
   menus).
   - With the rectangle selected, open the blend menu in Appearance: FigCraft's modes in groups, the
     current one, "Normal", checked. Pick Multiply, then press R without clicking the canvas: the
     Rectangle tool comes on. Do the same after pressing Escape in an open blend menu, after picking
     a stroke position (Inside, Center or Outside), and after picking an export format.
   - Select the Trail card, and do the same with its W and H menus (Fixed and Hug).
5. **The colour picker** (Step 3, and the P5.T5 ruling).
   - (a) Draw a rectangle (R, then drag): its fill is `D9D9D9`. Click the fill's swatch in the
     Design panel. The picker opens under the swatch, about 234 px wide, whole and inside the
     window. Drag its sliders: the canvas follows. Close it by clicking the swatch again, and press
     ⌘Z once: the fill has the colour it had before the pick. A second ⌘Z undoes the step before the
     pick, which is drawing the rectangle.
   - (b) Open the swatch again, drag a slider, and press Escape: the picker closes and the colour
     goes back. ⌘Z then undoes no colour.
   - (c) Open the picker, pick a colour, then press on the selected layer on the canvas and drag
     it: the picker closes, the layer moves, and no toast shows. ⌘Z twice undoes the move, then the
     colour. A click on empty canvas while a picker is open closes it, with no toast.
   - Type `ff0000` in the hex field next to the swatch and press Enter: the fill is red. Type `zzz`:
     the field shows the colour again.
6. **A drop shadow** (Step 3, and the P5.T5 ruling).
   - With the rectangle selected, click "+" in Effects. A "Drop shadow" row appears, and the canvas
     shows the shadow. Click the sliders button at the left of the row: x, y, blur, spread and the
     colour row show under it. Type 24 in blur: the shadow softens. Type -3: the field shows 24
     again, with no toast. Open the kind menu: Inner shadow and Layer blur (which has only a radius)
     and Background blur. The eye hides the effect, and "−" removes it. Each one is a ⌘Z step.
   - Select the Trail card: two "Drop shadow" rows, the top one first. Change one's blur: the other
     doesn't change. Open the editor of one of them, then remove the other: the editor should stay
     with the effect it was opened for (Task 5's deferred minor: editors are kept by place in the
     list, so it may jump to the effect that moves into the place).
7. **A font change** (Step 3, and the P5.T5 ruling).
   - Select "P5T6 text". Open the family menu: a search field over a list at most 320 px tall, long
     names cut with an ellipsis, the list filled once the font scan is done. Type part of a name,
     and pick a family with Enter or a click: the text changes to it. The style menu then lists
     that family's styles: pick Bold, and the text turns bold.
   - Then press R without clicking the canvas: the Rectangle tool comes on. Open the family list
     again, press Escape, press R: the same.
   - Try size 30, line height `auto`, `120` and `150%`, and letter spacing `-1` and `2%`. Each
     applies; `abc` and a negative line height are refused, and the field shows the old value.
     The marked align and resize buttons follow the text.
   - A text from the example's phone frames reads "Inter" in the family menu while the canvas draws
     it in Helvetica Neue. That is known (P2.T7's ruling), not a failure.
8. **Export 2× PNG** (Step 3, and P5-C12).
   - Select the probe (rows PNG 2× and SVG 1×), and click "Export 1 layer". Finder opens with the
     first file selected, in `<project>/.nyawit/exports/`: `P5T6 probe@2x.png` and `P5T6
     probe.svg`. The PNG is about 492 × 332 px (440 × 280 and the shadow's room).
   - Click the button again: new files named `P5T6 probe@2x-2.png` and `P5T6 probe-2.svg`, and the
     first two are untouched.
   - Select a layer with no export row: it shows "PNG 1×", and the button exports it so. Change
     that row's scale to 2×: it reads "PNG 2×", and the layer now holds the row (⌘Z takes it out).
     Click "+": a second row appears. Select two layers: the button reads "Export 2 layers".
9. **⌥8 on the real keyboard** (the P5.T4 ruling). Click the canvas, and press ⌥8: the Design panel
   hides, and the top bar's Design toggle loses its mark. Press ⌥8 again: both come back. Click
   the toggle itself: the same. Over the socket the key op did this both times.
10. **The panel's fold** (Step 1, and the P5.T4 ruling). Select the Trail card, with the window as
    tall as the screen. Scroll the panel with two fingers or the wheel: it scrolls and the canvas
    doesn't. At the bottom the Export row and the whole "Export 1 layer" button show above the
    window's edge. No scrollbar is drawn, as in Sawit's other side panels.
11. **Auto layout and align** (Step 1, and the P5.T3 and P5.T4 reviews).
    - Select the Trail card. Click each direction button (vertical, horizontal, wrap): its children
      line up that way. Click a cell of the 3 × 3 grid: they align there. Scrub the gap and the
      padding labels. Click the button beside the padding field: four fields, one for each side.
      Type 12 in the top one: only the top changes.
    - Draw a frame with the Frame tool and put two rectangles in it. Click "+" in the empty Auto
      layout header: the frame gets an auto layout, and the rectangles line up. "−" takes it away.
      One ⌘Z each.
    - Select three rectangles in different places. Click align-left, then distribute horizontally
      (the last two buttons are off with fewer than three layers): each is one ⌘Z step, and
      hovering a button shows its name and keys.
12. **A gradient row** (the P5.T5 ruling and its deferred minor). Select the Trail card's photo,
    which has a linear gradient (⌘-click it, or over the socket `ctl '{"cmd":"canvas","op":{"op":
    "command","command":"select.set","params":{"ids":[30]}}}'`): the row reads "Linear gradient".
    Click its replace-with-solid button: it becomes a solid, and ⌘Z brings the gradient back. Its
    swatch is a grey ramp, not the gradient's colours (the deferred minor): say whether that
    matters.
13. **How it looks and feels** (the P5.T4 and P5.T5 rulings).
    - Hover each kind of control: buttons, fields, menus. Focus a field: its ring. Say whether the
      panel looks like part of Sawit (its colours are the theme's), and whether anything is hard to
      hit, cramped or cut.
    - With Sawit's sidebar shown, drag the window's edge in to its 720 pt minimum. The panel keeps
      its 264 pt, and its fields stay whole. The canvas keeps the rest, and the toolbar scrolls
      inside it (Plan 3's hand check 6).

Write the answers here once the checks have run (R5).
