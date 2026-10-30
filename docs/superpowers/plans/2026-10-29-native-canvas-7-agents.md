# Native canvas 7: Agent parity, Mermaid, Excalidraw import, file tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Agents get everything they have today and more on the native canvas:
- all eight `canvas_*` MCP tools keep working, plus two new ones: `canvas_command` and
  `canvas_commands`
- Mermaid becomes editable board shapes with bound connectors
- old `.excalidraw` boards migrate on first open
- `.figcraft` and `.excalidraw` files open from the file tree in canvas tabs

**Architecture:**
- **One element format.** Element skeletons (the MCP JSON) are the shared intermediate. The agent's
  `add`, Mermaid (`mermaid.rs`, a Rust port of `web/canvas/src/pele.ts`) and Excalidraw scenes
  (`excalidraw.rs`) all become `Vec<Skeleton>`, and `skeleton::apply` turns that into board commands.
- **Ops.** `ops::run` implements the whole wire contract.
- **MCP.** `crates/mcp` keeps its tool names. It updates descriptions, adds the two tools, and reads
  `.figcraft` offline through `sawit-board`.

**Tech Stack:** As Plan 1, plus `roxmltree = "0.21"` and `kurbo = "0.13"`. Both are already in `Cargo.lock`, so no new versions. Mermaid rendering comes from `sawit_core::mermaid::render(source, &Palette::default())`.

**Spec:** `docs/superpowers/specs/2026-10-29-native-canvas-design.md` ("Agent wire contract", D5, D9, D10, D14)

## Global Constraints

- Toolchain: Rust 1.99.1 (`rust-toolchain.toml`), edition 2024, workspace member crates only.
- FigCraft deps are pinned in root `[workspace.dependencies]` to rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`.
- `crates/board` never depends on GPUI. All GPUI code lives in `crates/app/src/board/`.
- User-facing name stays "Canvas". The MCP tool names don't change.
- The native canvas is reachable only when `Workspace::native_canvas` is true. Plan 8 removes this. MCP descriptions change in Plan 8, not here.
- Engine errors become `{ok:false,error}`. No `unwrap`/`expect` outside tests.
- Gates before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the touched crates' tests.
- Commits:
  - Commit on `main` with explicit `git add <paths>`.
  - Conventional Commits with lowercase subjects. Every message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - **Never `git push`.**

## Review Focus

1. An agent id that looks like a node id (`"12"`) must resolve to the agent's element first. Test: `agent_ids_win_over_node_ids` (Task 1).
2. An arrow whose `start`/`end` id doesn't exist must become a free endpoint, not an error. Test: `arrow_to_missing_id_is_free` (Task 1).
3. Malformed input (a scene without `elements`, Mermaid syntax errors, unknown element types) must reply `ok:false` with a message or skip with `notes`. It must never panic. Tests: `bad_scene_is_reported`, `mermaid_syntax_error_is_reported`, `unknown_types_are_noted` (Tasks 3–4).
4. Migration must never touch the original `.excalidraw`, and must run only when no `.figcraft` exists. Test: `migration_imports_once_and_keeps_original` (Task 4).
5. `canvas_get` with the app not running must read `.figcraft`. With only an `.excalidraw` it must summarise that through the importer. Test: `offline_get_reads_figcraft_then_excalidraw` (Task 5).

---

### Task 1: Skeletons

**Files:**
- Create: `crates/board/src/skeleton.rs`
- Modify: `crates/board/Cargo.toml` (`serde = { version = "1", features = ["derive"] }`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Deserialize, Serialize, Clone, Debug, Default)]
  pub struct Skeleton {
      #[serde(rename = "type")] pub kind: String,   // rectangle|ellipse|diamond|sticky|text|arrow|line|frame|image|freedraw
      pub id: Option<String>, pub x: f64, pub y: f64, pub width: Option<f64>, pub height: Option<f64>,
      pub label: Option<Label>, pub text: Option<String>,
      pub start: Option<End>, pub end: Option<End>, pub points: Option<Vec<[f64; 2]>>,
      #[serde(rename = "strokeColor")] pub stroke_color: Option<String>,
      #[serde(rename = "backgroundColor")] pub background_color: Option<String>,
      pub roundness: Option<serde_json::Value>,
      #[serde(rename = "frameId")] pub frame_id: Option<String>,
      pub svg: Option<String>, pub data: Option<String>, pub name: Option<String>, // image: svg text or base64 data + name
  }
  pub struct Label { pub text: String }
  pub struct End { pub id: Option<String>, pub x: Option<f64>, pub y: Option<f64> }
  pub struct Applied { pub ids: Vec<String>, pub notes: Vec<String> }
  pub const SAWIT_ID: &str = "sawit.id";
  pub fn apply(board: &mut BoardDoc, items: &[Skeleton], offset: (f64, f64)) -> Result<Applied, BoardError>; // one undo step
  pub fn resolve(doc: &Document, id: &str) -> Option<NodeId>;   // pluginData["sawit.id"] == id first, then NodeId(id.parse())
  pub fn bounds(items: &[Skeleton]) -> Option<(f64, f64, f64, f64)>;
  ```
- The type mapping and colour handling are exactly the spec's "Skeleton" rules.
  - **Order:** shapes and frames first, then text, then arrows and lines, so bindings resolve.
  - **Agent ids:** an agent `id` is written with `node.set {ids, props:{pluginData:{"sawit.id": id}}}`.
  - **Undo:** the whole call is one undo step, made with `session.grouped("Add elements", ..)`.
  - **Result:** `ids` is the node id string of each top-level element, in input order.

- [ ] **Step 1: Write the failing tests:**
  - `rectangle_with_label_becomes_board_shape`: one rectangle `{label:{text:"API"}}` → 1 layer with `BoardItem::Shape{Rectangle}` and text "API".
  - `roundness_makes_rounded_rectangle`.
  - `arrow_binds_to_shapes_by_agent_id`: a, b, then an arrow from a to b → a connector whose endpoints are attached to a and b. Read this through `summary` in Task 2, or directly through `BoardItem::Connector` fields.
  - `arrow_to_missing_id_is_free`: `end:{id:"zzz"}` with x/y → a connector with a free end, and `notes` empty.
  - `agent_ids_win_over_node_ids`: add `{id:"1"}`; `resolve(doc, "1")` → that element even if `NodeId(1)` exists.
  - `frame_children_move_inside`: a frame f plus a rectangle with `frameId: "f"` → the rectangle's parent is the section.
  - `add_is_one_undo_step`: 3 elements, one `edit.undo` → the page is empty.
  - `line_with_many_points_is_ink`: `points` of length 4 → `BoardItem::Ink`, or the ink representation `board.ink` creates.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board skeleton::`
Expected: FAIL.

- [ ] **Step 3: Implement `skeleton.rs`.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board skeleton::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board
git commit -m "feat(canvas): apply element skeletons as board shapes, connectors, text and sections"
```

### Task 2: Summaries and the full set of ops

**Files:**
- Create: `crates/board/src/summary.rs`
- Modify: `crates/board/src/ops.rs` (`get`, `add`, `update`, `delete`, `clear`, `replace`, `export_svg`, `export_png`, `commands`; `pointer`, `key` and `type` already exist)

**Interfaces:**
- Consumes: `skeleton::{apply, resolve, Skeleton}`, `export_page` (Plan 6).
- Produces:
  ```rust
  pub fn summarize(doc: &Document, page: NodeId) -> Vec<serde_json::Value>; // spec Summary for each top-level layer (connectors included)
  pub fn element_type(doc: &Document, id: NodeId) -> &'static str;           // spec type list
  ```
- `update` accepts these fields:
  - `x`, `y`, `width`, `height` → `node.set`
  - `text` → the label text through `board.label` for connectors, else `node.set {characters}` on the
    text child
  - `backgroundColor` → `board.setColor`
  - `strokeColor` → `node.set {stroke}`
- `delete` removes connector labels with their connector.
- `clear` deletes every top-level layer (one undo step).
- `replace {scene}` clears the page, then imports through `excalidraw::to_skeletons` (Task 4).
- `replace {path}` reads `.figcraft` through `store::open` and swaps `board.session`, or imports an
  `.excalidraw` file.
- `commands` comes from `figcraft_engine::cmd::all_commands()`, minus the hidden ids.

- [ ] **Step 1: Write the failing tests** (`ops.rs`, `summary.rs`):
  - `get_summarises_types_and_text`: a sticky "hi", a diamond and a connector between them → types `sticky`, `diamond`, `connector`; the connector has `from`/`to`.
  - `add_returns_ids_in_order`.
  - `update_moves_and_relabels`: `{id, x:500, text:"New"}` → `updated:1`, and `get` shows x 500 and text "New".
  - `update_reports_missing`: an unknown id → `missing:["nope"]`.
  - `delete_removes_connector_label`.
  - `clear_is_one_undo_step`.
  - `export_png_is_base64_png` and `export_svg_has_svg_root`.
  - `commands_hide_hidden_ids`: there is no `file.new` in the list.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board ops:: summary::`
Expected: FAIL.

- [ ] **Step 3: Implement** `summary.rs` and the ops.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src
git commit -m "feat(canvas): get, add, update, delete, clear, replace and export ops on the native canvas"
```

### Task 3: Mermaid to editable shapes

**Files:**
- Create: `crates/board/src/mermaid.rs`
- Create: `crates/board/tests/fixtures/mermaid/flow.mmd`, `sequence.mmd`, `pie.mmd`
- Modify: `crates/board/Cargo.toml` (`roxmltree = "0.21"`, `kurbo = "0.13"`)
- Modify: `crates/board/src/ops.rs` (`mermaid` op)

**Interfaces:**
- Produces:
  ```rust
  pub const EDITABLE: &[&str] = &["flowchart", "sequence", "class", "state", "er", "mindmap", "requirement", "block", "kanban", "c4", "gitGraph", "timeline"]; // copy pele.ts:33 verbatim
  pub fn to_skeletons(d: &sawit_core::mermaid::Diagram) -> Result<Vec<Skeleton>, String>;
  // non-EDITABLE kinds → one Skeleton { kind: "image", svg: Some(d.svg), name: Some("diagram.svg"), x: 0, y: 0, width, height }
  // ops: "mermaid" {source, x?, y?} → render (Palette::default()); retry once with '"' → '\''; place right of existing content + 100 px gap, or at x/y; apply
  ```
- **Port.** `to_skeletons` ports the `Converter` class in `web/canvas/src/pele.ts:150-858`. It keeps the
  same passes: clusters/frames/columns, nodes/commits/notes, edges, then loose shapes and text.
- **Geometry.** DOM geometry becomes our own:
  - parse `transform` (translate, scale, rotate, matrix) and compose it down the tree with
    `kurbo::Affine`
  - bounding boxes: `rect`, `circle`, `ellipse`, `line`, polygon/polyline points, and `path` through
    `kurbo::BezPath::from_svg(..).bounding_box()`
- **Labels.** A text's position is its `x`/`y` with `text-anchor`. Width doesn't matter, because the
  engine lays out the label text.
- **Background.** The background `rect` that `flatten` adds is skipped.
- **Colours.** `fill` → `backgroundColor`, `stroke` → `strokeColor`.

- [ ] **Step 1: Write the failing tests** (fixtures rendered with `sawit_core::mermaid::render`):
  - `flowchart_gives_nodes_and_bound_arrows`: `flowchart LR; A[Start]-->B{Ok?}; B-->|yes|C[Done]` → 3 node skeletons (one diamond) and 2 arrows whose `start.id`/`end.id` name node skeletons; one arrow has label "yes".
  - `sequence_is_editable`: `sequence.mmd` → ≥ 2 rectangles and ≥ 1 arrow.
  - `pie_comes_in_as_svg_image`: `kind == "image"` and `svg` starts with `<svg`.
  - `mermaid_syntax_error_is_reported`: the op with `"flowchart LR; A-->"` → `ok:false` and an `error` starting `"mermaid: "`.
  - `mermaid_op_places_right_of_content`: one existing sticky at x 0..200 → the new content's min x ≥ 300.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board mermaid::`
Expected: FAIL.

- [ ] **Step 3: Implement `mermaid.rs` and the op.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board
git commit -m "feat(canvas): turn mermaid into editable board shapes without a browser"
```

### Task 4: Excalidraw import and migration

**Files:**
- Create: `crates/board/src/excalidraw.rs`
- Create: `crates/board/tests/fixtures/excalidraw/basic.excalidraw` (rectangle with bound text, ellipse, diamond, bound arrow, free line, freedraw, standalone text, frame with child, image with dataURL, embeddable)
- Modify: `crates/board/src/store.rs` (`open` migrates)

**Interfaces:**
- Produces:
  ```rust
  pub struct Imported { pub skeletons: Vec<Skeleton>, pub notes: Vec<String> }
  pub fn to_skeletons(scene: &serde_json::Value) -> Result<Imported, String>; // Err when there is no `elements` array
  pub fn import(board: &mut BoardDoc, scene: &serde_json::Value) -> Result<Vec<String>, BoardError>; // apply at offset (0, 0); returns notes
  // store.rs: open(path) → when !path.exists() and path.with_file_name("canvas.excalidraw") exists → new board + import; never writes either file
  ```
- **Excalidraw mapping:**
  - **Skipped:** deleted elements (`isDeleted`).
  - **Shape labels:** bound text (`containerId`) becomes its container's `label`.
  - **Arrow ends:** `startBinding.elementId` / `endBinding.elementId` become `start`/`end` ids.
  - **Arrow points:** `x + points[i]`.
  - **Images:** `fileId` looks up `files[fileId].dataURL`, which becomes `data` (base64) + `name`.
  - **Freedraw:** becomes `freedraw` with its points.
  - **`embeddable`, `iframe`, `magicframe`:** go to `notes` (`"skipped 1 embed"`).
- **Migration:**
  - It leaves `canvas.excalidraw` untouched.
  - The first autosave writes `canvas.figcraft`.

- [ ] **Step 1: Write the failing tests:**
  - `basic_scene_imports_everything_supported`: on `basic.excalidraw` → the counts by summary type match the fixture (1 rectangle labelled, 1 ellipse, 1 diamond, 1 connector bound at both ends, 1 free connector or ink, 1 ink, 1 text, 1 section holding 1 child, 1 image); `notes == ["skipped 1 embed"]`.
  - `bad_scene_is_reported`: `{}` → `Err`.
  - `unknown_types_are_noted`: an element of type `"laser"` → skipped with a note.
  - `migration_imports_once_and_keeps_original`: a temp `.nyawit` with only `canvas.excalidraw` → `open` gives the imported layers. The `.excalidraw` bytes are unchanged and `canvas.figcraft` doesn't exist until a save. After a save, `open` reads the `.figcraft` and doesn't import again (add one sticky, save, reopen → the sticky is still there).

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board excalidraw:: store::`
Expected: FAIL.

- [ ] **Step 3: Implement** `excalidraw.rs` and the migration.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board
git commit -m "feat(canvas): import excalidraw boards and migrate the project canvas on first open"
```

### Task 5: MCP tools and the app control path

**Files:**
- Modify: `crates/mcp/Cargo.toml` (`sawit-board = { path = "../board" }`)
- Modify: `crates/mcp/src/app.rs`:
  - add `canvas_command` (`CanvasCommandArgs { command: String, params: Option<Value>, project }`) and
    `canvas_commands`
  - `canvas_load` accepts `.figcraft` paths
  - offline `canvas_get` reads `sawit_board::store::open(board_file)` + `summary::summarize`, and falls
    back to the old Excalidraw summary only when neither file is present
- Modify: `crates/mcp/src/lib.rs` (the expected tool-name list in the test; the server instructions gain one line on `canvas_command`)
- Modify: `crates/app/src/workspace/control.rs` (the native branch already answers `Answer::Now(run_op)`; nothing else changes)

**Interfaces:**
- Consumes: `ops::run`, `store::{board_file, open}`, `summary::summarize`.
- Produces:
  - MCP tools `canvas_command {command, params?, project?}` → op `command`, and
    `canvas_commands {project?}` → op `commands`.
  - When the app answers on the control socket, every canvas tool goes through it, as today. Only when
    no app is running do they run headless on the stored file through `store::open` + `ops::run` +
    `Writer`. This is how agents edit canvases while Sawit is closed, and it never races the app's
    autosave.

- [ ] **Step 1: Write the failing tests:**
  - `crates/mcp/src/lib.rs`: the tool-name list includes `canvas_command` and `canvas_commands`, and every tool has a description (the existing test, updated).
  - `crates/mcp/tests/tools.rs`:
    - `offline_get_reads_figcraft_then_excalidraw`: a temp project with `.nyawit/canvas.figcraft` holding 2 stickies → `canvas_get` returns 2 elements. With only a `canvas.excalidraw` it returns the imported elements.
    - `offline_command_edits_the_file`: `canvas_command {command:"board.sticky", params:{x:0,y:0}}` with no app → the file now has 1 sticky.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-mcp`
Expected: FAIL.

- [ ] **Step 3: Implement** the tools and the offline paths.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-mcp && cargo test -p sawit`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/mcp crates/app/src/workspace/control.rs Cargo.lock
git commit -m "feat(mcp): canvas_command and canvas_commands, and canvas tools that work on .figcraft files"
```

### Task 6: `.figcraft` and `.excalidraw` files open in canvas tabs

**Files:**
- Modify: `crates/core/src/files.rs` (`FileKind::Figcraft` for `figcraft`, `FileKind::Excalidraw` for `excalidraw`; a `classify` test)
- Modify: `crates/app/src/workspace.rs` (`open_file`: these kinds open a `BoardView` tab, key `file\0<path>`, icon `Lucide::Shapes`, when `native_canvas`; otherwise they keep today's behaviour)
- Modify: `crates/app/src/board/mod.rs` (`pub fn import(path: PathBuf, cx: &mut Context<Self>) -> Self` for `.excalidraw`: imported board bound to the sibling `<stem>.figcraft`; an existing sibling opens instead)

**Interfaces:**
- Consumes: `store::open`, `excalidraw::import`.
- Produces: `FileKind::{Figcraft, Excalidraw}`, `BoardView::import`.

- [ ] **Step 1: Write the failing tests:**
  - `classify_figcraft_and_excalidraw` (core).
  - `excalidraw_file_opens_bound_to_sibling` (`#[gpui_kit::test]`): `BoardView::import("<tmp>/x.excalidraw")` → `path() == <tmp>/x.figcraft`; after `flush` the `.figcraft` exists and the `.excalidraw` is unchanged.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-core files && cargo test -p sawit board::`
Expected: FAIL.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-core && cargo test -p sawit`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/files.rs crates/app/src/workspace.rs crates/app/src/board/mod.rs
git commit -m "feat(canvas): open .figcraft and .excalidraw files in canvas tabs"
```

### Task 7: Live verification over MCP

- [ ] **Step 1:** Launch with the flag. Put an old `canvas.excalidraw` (copy one from a real project's `.nyawit/`, or the Task 4 fixture) in the scratch project.
  - Open the canvas.
  - Expected: the screenshot shows the migrated content.
- [ ] **Step 2: Check Mermaid over JSON-RPC.**
  - Run `sawit mcp` (the binary invoked as MCP; see `invoked_as_mcp` in `main.rs`) and call `canvas_add_mermaid` with the Task 3 flowchart.
  - Then call `canvas_get`.
  - Expected: 3 nodes plus 2 connectors with `from`/`to`.
  - Screenshot: editable shapes, with connectors attached.
- [ ] **Step 3:** Call `canvas_command {command:"board.template", params:{id:<first from board.templates>}}`, then `canvas_export {format:"png"}`.
  - Expected: a PNG under `.nyawit/exports/`.
- [ ] **Step 4: Stop the instance** with `/bin/kill`, then run the full gates:
Run: `cargo test --workspace`
Expected: all pass.

#### Task 7 notes: live check (2026-10-31, at `a0bcf46`, lane 3; the fix and the gates at `78b7f34`)

Steps 1–3 and every carried check passed after one fix. Migrating a board made by today's canvas
turned its Mermaid self-loops into stubs and bent its sequence messages onto notes; `78b7f34`
fixes the importer (below). Step 4: `cargo test --workspace` passed. What needs a person is
under "Hand checks".

How it ran:
- Debug build of the lane. Six launches per R-LAUNCH (`--foreground`, stderr to a log, a
  scratch `SAWIT_HOME`, stopped with `/bin/kill`), load average at each launch in brackets:
  1. the flag on, project `proj`: Steps 1–3, the routing carry, the Mermaid carry (4.67 8.11 7.85);
  2. the flag on, `real`: a copy of the real old board (1.58 3.45 5.59);
  3. the flag off, `legacy`: today's canvas (2.19 2.94 5.09);
  4. the flag on, `legacy`: that board migrated (3.45 3.49 4.98);
  5. the flag on, `legacy-fixed`: the same file migrated after the fix (2.98 2.40 3.35);
  6. the flag off, `real-legacy`: the real board as today's canvas draws it (4.15 3.02 3.39).
- MCP calls went to the lane's `target/debug/sawit-mcp` over stdio JSON-RPC (C18), with the same
  `SAWIT_HOME`, `SAWIT_NATIVE_CANVAS=1` (unset with the flag off) and `project` set. One offline
  call ran it as a symlink named `sawit-mcp` to `target/debug/sawit` (`invoked_as_mcp`).
- Everything else went over the control socket. No AX, osascript or posted events. The window
  kept Sawit's 1280 × 801; `layout` hid the sidebar and terminal for the closer screenshots.
  No change was seen that no op of this check made (P3.T4 carry).
- The board copies and screenshots stayed in the scratchpad (`live-P7.T7/`). The real board's
  copies and their screenshots were deleted after the check and are not committed.

Step 1 (the Task 4 fixture as `proj/.nyawit/canvas.excalidraw`): `open_page canvas` shows the
migrated board at 49 %: "Service" with its arrow to the ellipse, the diamond, the free line, the
freedraw, "Hello board", the "Group A" section with its rectangle, and the picture. A toast says
"canvas.figcraft: skipped 1 embed". `get` lists 9 top-level layers (section, rectangle "Service",
ellipse, diamond, image, text, connector 3→6, a free connector, ink). The first autosave wrote
`canvas.figcraft` (P7-C8), and `canvas.excalidraw` stayed byte-identical.

Carry (P7.T6, one view per file): copies of the fixture at `proj/x.excalidraw` and
`proj/.nyawit/canvas.excalidraw`. `open` of `x.excalidraw`, `x.figcraft`,
`.nyawit/canvas.excalidraw` and `.nyawit/canvas.figcraft`, in that order (absolute `/private/tmp`
paths), then `open_page canvas`, then `state`. Answer: exactly two tabs, one of kind `canvas` and
one of kind `file` (title `x.figcraft`, path `…/proj/x.figcraft`); no other tab for either file.
The import wrote `x.figcraft` at its first autosave, and both `.excalidraw` files are unchanged.

Carry (P7.T4 (1), the real old board): a copy of `~/Library/Application Support/sawit/
canvas.excalidraw` (167,774 bytes now, changed 2026-10-30 13:37, still 134 live elements) as
`real/.nyawit/canvas.excalidraw`. `get`, with the board unchanged: 31 roundedRectangle,
8 rectangle, 3 diamond, 2 text and 42 connector (6 with text, 38 joined at both ends, 4 at their
end only), as expected, and the copy's checksum is unchanged. The import leaves nothing out
(`canvas_load` of a copy into a scratch project answers 86 layers and no `notes`), and
`canvas_get` with the app closed answers the same 86 layers from a fresh copy and writes nothing.
What looks different from the board as today's canvas draws it (launch 6; that canvas also
follows the app's dark theme, where the native one keeps the document's light page, D12):
- The headings "sawit.dev — user flow" (36 px) and "Legend" (24 px) come in at 12 px.
- The 8 legend labels lose their category colours (#495057, #6741d9, #1971c2, #e03131, #f08c00,
  #0c8599, #2f9e44, #e8590c) and are drawn in ink; the boxes keep their fills and outlines.
- The 10 texts in Excalidraw's hand-drawn font (the headings and the legend) and the 40 in
  Nunito all come in in the engine's face; label sizes (14, 16 and 20 px) become the engine's,
  and some labels break at other words ("Pick version, port, data dir").
- The 30 edges written with 10–19 points along Pele's routes come in straight. The edges into a
  box meet at one point of it (the four into "Workspace" fan in as long diagonals, where today
  they come down as verticals), and "install script" crosses the "Right-click Open / xattr" box.
- 59 elements carry Excalidraw's `roughness` 1; it isn't carried.

Step 2 (`sawit-mcp`, the Task 3 flowchart): `canvas_get` first (9 layers), then
`canvas_add_mermaid {source: "flowchart LR; A[Start]-->B{Ok?}; B-->|yes|C[Done]"}` answers
`{ok, added: 5, ids: [20, 23, 26, 29, 30]}`, placed at x 1100, 100 right of the fixture.
`canvas_get`: 3 nodes (roundedRectangle "Start", diamond "Ok?", roundedRectangle "Done") and
2 connectors with `from`/`to`: 20→23, and 23→26 with text "yes". The screenshot (154 %) shows three
editable shapes and two arrowed connectors. They are attached: `canvas_update` moving "Done" 120
down rerouted connector 30 to its new top, and `canvas_command edit.undo` (`{undone: true}`) put it
back.

Carry (P7.T3, Mermaid parity): (a)–(d) drawn with `canvas_add_mermaid` at x 0, 1200, 2400, 3600
(y 1000), each with Pele's own picture of it (`render_mermaid`, light) placed 80 below through
`canvas_add_elements {type: image, svg, name: pele.svg}`, which comes in as an SVG frame. No reply
had notes. All four meet their expectations; the differences are:
- (a) `stateDiagram-v2` (`[*] --> Still`, `Still --> Still : tick`, `Still --> Moving`): the
  self-transition is a loop with its head and "tick" beside it. The loop and its head are ink,
  heavier than Pele's line; "tick" is 12 px to Pele's 14; the nodes are unfilled with ink
  outlines where Pele fills them grey with grey outlines (today's canvas maps Pele's neutral
  colours the same way, pele.ts `NEUTRAL_FILLS`).
- (b) `sequenceDiagram` (`Alice->>Bob: Hello`, `Note over Bob: Thinks`, `Bob-->>Alice: Hi`,
  `Note over Alice,Bob: Both`): both messages run level and end at the lifelines, and neither
  bends onto a note. "Hi" is solid where Pele dashes it, and its labels are 12 px. The lifelines
  are drawn over the notes and through "Thinks", as today's canvas draws them too (pele.ts adds
  them last).
- (c) `classDiagram` (`Animal <|-- Dog`, `Animal *-- Leg`, `Animal ..> Food: get`): `from`/`to`
  on all three edges, and the inheritance head and the composition diamond sit at Animal. The
  head is a filled triangle cap where Pele's is hollow, the diamond an ink outline where Pele
  fills it; the three straight edges meet at one point under Animal where Pele's curves leave it
  apart; "get" is solid where Pele dashes it; the class names are regular where Pele sets them
  bold.
- (d) `flowchart LR` (`A -.->|maybe| B`, `A ==> C`, `C --> C`): the self-loop is a loop and
  "maybe" is on its edge. A→B is solid where Pele dots it, A⇒C thin where Pele draws it thick, and
  both are straight where Pele curves them.
- With the flag off (launch 3), today's canvas draws the same four with Pele's curves, dashes and
  dots, a hollow inheritance triangle, labels at 14 px in Nunito and larger Excalidraw heads.

Carry (P7.T4 (2), migrating today's file): launch 3 also drew `erDiagram` /
`CUSTOMER ||--o{ ORDER : places` and `flowchart LR` / `A --- B` / `B <--> C` beside (c). Today's
canvas shows bars and a crow's foot on the ER line, no marker on `A --- B` and heads at both ends of
`B <--> C`. The app was stopped once `legacy/.nyawit/canvas.excalidraw` held all 71 elements, and
launch 4 migrated it (48 layers; the file unchanged). `A --- B` is a connector with no caps; the
class heads stay at Animal; the composition diamond and the crow's foot come in as triangle end
caps; `B <--> C` and the ER line keep only their end head; dashes and dots come in solid and curves
straight (recorded, not failed). It also showed a defect: both self-loops were stubs (a head on the
node's edge and no loop), "Hello" bent down onto "Thinks", and "Hi" ran out of the "Both" note.

Defect, fixed in `78b7f34` ("fix(canvas): migrated loops and sequence messages stay as
excalidraw drew them"):
- Cause: today's canvas (pele.ts through Excalidraw) bound a sequence message's end to the note
  Pele puts just under it (`focus` ±1.61, gap 10: the line runs beside the note), and bound a
  loop back to one node to that node at both ends. Excalidraw draws the stored points, so both
  look right today. The importer joined every bound end, so the engine routed each message onto
  its note, and a connector from a node to itself became a stub (`skeleton::apply` frees one end
  of such a connector, with a note). `mermaid.rs` handles both cases on the native path already.
- Fix (`crates/board/src/excalidraw.rs`): an end stays free where it is drawn when it runs beside
  the element it is bound to, that is when its binding's `focus` is outside -1..1 and the line,
  carried on past that end, misses the element's box (turned by its angle, as Excalidraw measures
  a focus); a line bound to one element at both ends joins neither, and such an arrow with no
  label is ink along its route with its triangle head drawn in it (Excalidraw's size: half the
  last step, at most 15, 25° to each side). Every other binding in the two files seen here has
  |focus| ≤ 0.76, and the real board migrates exactly as before (same counts after the fix).
- Review fix round 1 added the box test. Excalidraw 0.18.1's `determineFocusDistance` (minified
  `pg` in `crates/app/canvas/chunks/chunk-W2HEY36P.js`) divides a diamond's axis crossing by the
  axis at its index after filtering and sorting, so an arrow straight down into the top of a
  289 × 123 diamond is written with focus 1.138 at 70 off its centre and 1.626 at 100, though it
  meets the diamond. With the focus alone it came in free; now it joins. A focus up to ±1 (or none)
  still joins wherever the line is drawn, since today's canvas moved only two-point arrows along
  when a script moved a shape. Re-checked: the six-diagram file migrates element for element as
  at launch 5, and a copy of the real board gives the same counts with no notes.
- Tests (RED before the fix, GREEN after):
  `an_end_bound_to_an_element_it_runs_beside_stays_where_it_is_drawn`,
  `an_arrow_bound_to_one_element_at_both_ends_joins_neither`,
  `a_loop_back_to_one_element_is_drawn_along_its_route_with_its_head`, and from the fix round
  `an_arrow_into_a_diamond_joins_it_whatever_its_focus_says`.
- Live, launch 5 (the same file, a fresh project): both loops are loops with their heads, and
  "Hello" and "Hi" run level from lifeline to lifeline as today's canvas draws them: 22 connectors
  and 2 ink, where launch 4 had 24 connectors. `canvas_load` of the file answers no `notes`.

Step 3: `board.templates` lists retrospective, brainstorm, weeklyPlanner, flowChart and kanban.
`canvas_command board.template {name: "retrospective"}` (C19) answers `{name: "retrospective",
ids: [234, 235, 240, 245]}` and places the title and its three sections (1280 × 592), centred in
the view. `canvas_export {format: "png"}` wrote `proj/.nyawit/exports/canvas-1793397241699.png`
(325,375 bytes, a 3821 × 2552 PNG of the whole page) and made `.nyawit/.gitignore`. With the app
closed, `canvas_get` read all 65 layers back from `canvas.figcraft`.

Step 4: every instance stopped with `/bin/kill`. `cargo test --workspace` at `78b7f34`: 1294
passed, 9 ignored, 0 failed (app 263, sawit-board 623, sawit-core 324 + 23, sawit-mcp 61).

Not failures, for the final review:
- `canvas_command` answers the engine's own result, so `board.template`'s ids are numbers
  (`[234, …]`) where `get` and `add` give strings (`"234"`).
- Launch 5 took over 4 s to listen; the first two requests were refused and resent.

##### Hand checks (pending — needs the user)

Run `SAWIT_NATIVE_CANVAS=1 target/debug/sawit` on a scratch project, open the Canvas page, and
have an agent call `canvas_add_mermaid` for each source below, with `render_mermaid` (light) of
the same source placed under it. Then quit, start `target/debug/sawit` without the flag on another
scratch project and draw the same sources there. Record each answer under its number.

1. Mermaid parity (P7.T3 carry). Sources: (a) `stateDiagram-v2` / `[*] --> Still` /
   `Still --> Still : tick` / `Still --> Moving`; (b) `sequenceDiagram` /
   `Alice->>Bob: Hello` / `Note over Bob: Thinks` / `Bob-->>Alice: Hi` /
   `Note over Alice,Bob: Both`; (c) `classDiagram` / `Animal <|-- Dog` / `Animal *-- Leg` /
   `Animal ..> Food: get`; (d) `flowchart LR` / `A -.->|maybe| B` / `A ==> C` / `C --> C`.
   Compare each pair, and the flag-off drawing, and choose which Skeleton fields to add for
   parity: `fontSize` for texts and labels, label colour, `strokeStyle` and `strokeWidth`, a start
   cap and the kind of each cap, curved or routed bound edges. None of them fails the task.
   Answer: ______
2. Import losses (P7.T4 carry (3)). Copy a real old board (never the original), for example
   `~/Library/Application Support/sawit/canvas.excalidraw`, to `<scratch>/.nyawit/
   canvas.excalidraw`. Open it once without the flag, then once with it (it migrates), and say
   which losses matter: text sizes (headings come in at 12 px), label colours (the legend),
   fonts (Excalidraw's hand-drawn one), marker kinds and a head at each end (crow's feet, bars,
   diamonds and hollow triangles become one filled triangle at the end), dashes and widths,
   curved routes, and, for boards drawn by hand, rotation, opacity, fill style, roughness and
   groups. Answer: ______
