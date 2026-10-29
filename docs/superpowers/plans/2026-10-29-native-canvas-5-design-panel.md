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
