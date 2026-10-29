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
