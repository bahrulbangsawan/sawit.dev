# Native canvas 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A view-only native canvas tab (FigCraft document rendered by the engine, pan/zoom, dot grid, autosave) behind `SAWIT_NATIVE_CANVAS=1`, plus the `get` and `command` control ops.

**Architecture:** A new GPUI-free crate `crates/board` (`sawit-board`) owns the FigCraft session, file
store and view maths. The app gets `crates/app/src/board/`, with a render worker thread that turns
`figcraft_render::Retained` output into BGRA `RenderImage`s and a `BoardView` entity that paints them and
re-projects the last frame while panning or zooming. `Workspace::open_canvas` picks `BoardView` when the
flag is set.

**Tech Stack:** Rust 1.99.1, GPUI via gpui-kit 0.8.1 (gpui-pre 0.3.10), FigCraft rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`.

**Spec:** `docs/superpowers/specs/2026-10-29-native-canvas-design.md`

## Global Constraints

- Toolchain: Rust 1.99.1 (`rust-toolchain.toml`), edition 2024, workspace member crates only.
- FigCraft deps are pinned in root `[workspace.dependencies]`:
  `{ git = "https://github.com/storytold/figcraft", rev = "0971d096db18aa107c5f3d1f83af4ac7f89ea2c3" }`.
  Never use a branch or an unpinned dependency.
- `crates/board` never depends on GPUI. All GPUI code lives in `crates/app/src/board/`.
- User-facing name stays "Canvas" (title `crate::canvas::TITLE`, icon `Lucide::Shapes`).
- The native canvas is reachable only when `Workspace::native_canvas` is true (env `SAWIT_NATIVE_CANVAS` set). Plan 8 removes this.
- Canvas file: `<project>/.nyawit/canvas.figcraft`, else `<AppPaths root>/canvas.figcraft`.
- Engine errors become a toast or an `{ok:false,error}` reply. No `unwrap`/`expect` on engine or I/O results outside tests.
- Gates before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the touched crates' tests.
- Commits:
  - Commit on `main` with explicit `git add <paths>`, never `-A`.
  - Conventional Commits with lowercase subjects. Every message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - **Never `git push`.** Pushing `main` runs the release hook.

## Review Focus

1. A corrupt or truncated `canvas.figcraft` must show an error placeholder in the tab, and autosave must never overwrite it. Test: `open_corrupt_file_errors_and_never_saves` (Task 2) and `corrupt_file_shows_error_and_leaves_file` (Task 5).
2. A zero-size canvas, e.g. the tab mid-layout or the window minimised, must not queue a render job or panic. Test: `render_view_is_none_for_empty_viewport` (Task 3).
3. A save into a missing `.nyawit/` must create it with its `.gitignore`. A failed save must surface its error and keep the in-memory document. Tests: `writer_creates_nyawit_with_gitignore`, `writer_reports_errors` (Task 2).
4. Several quick saves must leave the newest content, and a save that empties the canvas must leave a `.bak`. Tests: `newest_save_wins`, `emptying_keeps_backup` (Task 2).
5. Zoom at the extremes (0.02, 256) and non-finite deltas from trackpads must leave the view finite. Test: `zoom_at_clamps_and_ignores_non_finite` (Task 3).

---

### Task 0: Preflight

- [ ] **Step 1: Check the tree is clean**

Run: `git status --porcelain | grep -v -e '^?? docs/superpowers/' -e '^?? .claude/worktrees/'`
Expected: no output. Otherwise stop and ask the user to commit their in-progress work, e.g. with `/consolidate`. The plans edit the same files.

Then commit the spec and plans on their own:
`git add docs/superpowers/specs/2026-10-29-native-canvas-design.md docs/superpowers/plans/2026-10-29-native-canvas-*.md && git commit -m "docs: spec and plans for the native figcraft canvas"` (with the attribution line).

- [ ] **Step 2: Baseline gates**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass. Record any pre-existing failure in this plan before going on.

- [ ] **Step 3: Baseline release size** (Plan 8 compares against it)

Run: `cargo build --release -p sawit && ls -l target/release/sawit`
Expected: the build succeeds. Record the byte size here.

> **Baseline (2026-10-29, at `0f5df5e`):** fmt, clippy and `cargo test --workspace` all pass
> (449 passed, 9 ignored, no pre-existing failures). `target/release/sawit` is **35,670,592 bytes**.

### Task 1: `sawit-board` crate and `BoardDoc`

**Files:**
- Modify: `Cargo.toml` (`members` += `"crates/board"`; `[workspace.dependencies]` += `figcraft-engine`, `figcraft-render`, `figcraft-doc`, `figcraft-geom`, `figcraft-text`, `figcraft-format`, `figcraft-color`, all pinned per Global Constraints; `serde_json = "1"`)
- Create: `crates/board/Cargo.toml` (package `sawit-board`, deps: the seven figcraft crates, `serde_json`, `sawit-core = { path = "../core" }`, `log`; dev-dep `tempfile = "3"`)
- Create: `crates/board/src/lib.rs` (`pub mod doc; pub mod store; pub mod view; pub mod ops;` plus re-exports `pub use doc::{BoardDoc, BoardError, DocKind};`)
- Create: `crates/board/src/doc.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum BoardError { Engine(String), Format(String), Io(String) } // Display, Error, Clone, PartialEq
  impl From<figcraft_engine::EngineError> for BoardError
  pub enum DocKind { Board, Design }
  pub struct BoardDoc { pub session: figcraft_engine::Session }
  impl BoardDoc {
      pub fn new_board(name: &str) -> Result<Self, BoardError>;      // Session::empty() + "file.newBoard" {name}
      pub fn from_bytes(bytes: &[u8]) -> Result<Self, BoardError>;   // figcraft_format::load_bytes + Session::with_document
      pub fn to_bytes(&self) -> Result<Vec<u8>, BoardError>;         // figcraft_format::save_bytes(&doc, None)
      pub fn doc(&self) -> Result<std::sync::Arc<figcraft_doc::Document>, BoardError>;
      pub fn kind(&self) -> DocKind;                                 // figcraft_engine::board::is_board
      pub fn revision(&self) -> u64;                                 // ds().revision, 0 without a doc
      pub fn execute(&mut self, id: &str, params: &serde_json::Value) -> Result<serde_json::Value, BoardError>;
  }
  ```

- [ ] **Step 1: Write the failing tests** in `crates/board/src/doc.rs` (`#[cfg(test)] mod tests`):

```rust
#[test]
fn new_board_is_a_board_with_an_empty_page() {
    let b = BoardDoc::new_board("Canvas").unwrap();
    assert_eq!(b.kind(), DocKind::Board);
    let doc = b.doc().unwrap();
    let page = doc.first_page().unwrap();
    assert!(doc.node(page).unwrap().children.is_empty());
}

#[test]
fn bytes_round_trip_keeps_a_sticky() {
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    b.execute("board.sticky", &json!({"x": 0, "y": 0, "text": "hi"})).unwrap();
    let before = b.doc().unwrap().nodes().count();
    let back = BoardDoc::from_bytes(&b.to_bytes().unwrap()).unwrap();
    let doc = back.doc().unwrap();
    assert_eq!(doc.nodes().count(), before);
    assert!(doc.nodes().any(|n| matches!(n.board, Some(figcraft_doc::BoardItem::Sticky { .. }))));
}

#[test]
fn unknown_command_is_an_engine_error() {
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    assert!(matches!(b.execute("no.such", &json!({})), Err(BoardError::Engine(_))));
}

#[test]
fn revision_moves_on_edit() {
    let mut b = BoardDoc::new_board("Canvas").unwrap();
    let r0 = b.revision();
    b.execute("board.sticky", &json!({"x": 0, "y": 0})).unwrap();
    assert!(b.revision() > r0);
}
```

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board doc::`
Expected: FAIL to compile (`BoardDoc` missing).

- [ ] **Step 3: Implement `doc.rs`** with the signatures above.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board doc::`
Expected: 4 passed.

- [ ] **Step 5: Commit**
```bash
git add Cargo.toml Cargo.lock crates/board
git commit -m "feat(canvas): add the sawit-board crate around the figcraft engine"
```

### Task 2: Store: file location, open, autosave writer

**Files:**
- Modify: `crates/core/src/nyawit.rs` (add `pub fn board_file(project: &Path) -> PathBuf`, returning `dir(project).join("canvas.figcraft")`, and a test beside the existing ones)
- Create: `crates/board/src/store.rs`

**Interfaces:**
- Consumes: `BoardDoc::{new_board, from_bytes, to_bytes, doc}` (Task 1).
- Produces:
  ```rust
  pub fn board_file(project: Option<&Path>, app_root: &Path) -> PathBuf;
  pub fn open(path: &Path) -> Result<BoardDoc, BoardError>;   // missing file → new_board("Canvas"), nothing written
  pub fn is_empty(doc: &figcraft_doc::Document) -> bool;       // no layers on any page
  pub struct Writer;                                           // owns a "canvas-save" thread
  impl Writer {
      pub fn spawn() -> Self;
      pub fn save(&self, path: PathBuf, bytes: Vec<u8>, empty: bool); // newest save per path wins
      pub fn flush(&self);                                     // blocks until everything queued is on disk
      pub fn last_error(&self) -> Option<String>;
  }
  ```
- Rules:
  - Write `<path>.tmp`, then rename.
  - When `empty` is true and the file on disk loads as a non-empty document, copy it to `<path>.bak` first.
    This is the same rule as `canvas.rs::keep_backup_before_emptying`.
  - Parent dir named `.nyawit` → `sawit_core::nyawit::ensure(project, parent)`. Otherwise `create_dir_all`.

- [ ] **Step 1: Write the failing tests** (`store.rs` tests, `tempfile::tempdir()`):
  - `board_file_paths`: `board_file(Some(p), r) == p.join(".nyawit/canvas.figcraft")`; `board_file(None, r) == r.join("canvas.figcraft")`.
  - `open_missing_file_is_a_new_board_and_writes_nothing`: kind Board; `!path.exists()`.
  - `open_corrupt_file_errors_and_never_saves`: write `b"not a figcraft file"`, `open` → `Err(BoardError::Format(_))`; bytes on disk unchanged.
  - `writer_creates_nyawit_with_gitignore`: save into `<tmp>/.nyawit/canvas.figcraft`, `flush`; the file loads with `open`; `.nyawit/.gitignore` exists.
  - `newest_save_wins`: two `save`s to one path with different stickies, `flush`; `open` shows the second.
  - `emptying_keeps_backup`: save a non-empty doc, flush; save an empty doc with `empty = true`, flush; `<path>.bak` loads non-empty.
  - `writer_reports_errors`: save under a path whose parent is a regular file; flush; `last_error()` is `Some`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board store:: && cargo test -p sawit-core nyawit`
Expected: FAIL (missing items).

- [ ] **Step 3: Implement** `nyawit::board_file` and `store.rs`. The writer is an `mpsc` channel of `(PathBuf, Vec<u8>, bool)`; it drains the channel and keeps only the last item per path. `flush` sends a marker and waits for an ack.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board store:: && cargo test -p sawit-core nyawit`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/nyawit.rs crates/board/src/store.rs crates/board/src/lib.rs
git commit -m "feat(canvas): open and autosave .nyawit/canvas.figcraft with a backup before emptying"
```

### Task 3: View maths

**Files:**
- Create: `crates/board/src/view.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const OVERSCAN: f64 = 256.0;          // logical px per side
  pub const PREVIEW_QUALITY: f64 = 0.5;     // while zooming
  pub struct Viewport { pub width: f64, pub height: f64, pub scale_factor: f64 } // Copy, PartialEq
  pub fn render_view(view: &ViewState, vp: Viewport, quality: f64) -> Option<figcraft_render::View>;
  pub fn placement(frame: &figcraft_render::View, now: &ViewState) -> [f64; 4];   // x, y, w, h in logical px from the canvas origin
  pub fn dots(zoom: f64, page_rgb: [u8; 3]) -> Option<figcraft_render::Dots>;
  pub fn page_rgb(doc: &Document, page: NodeId) -> [u8; 3];                       // NodeKind::Page { background } → to_rgba8; white otherwise
  pub fn to_bgra(pixmap: &figcraft_render::Pixmap, page_rgb: [u8; 3]) -> Vec<u8>; // premultiplied RGBA composited over page_rgb, then R/B swapped
  pub fn pan(view: &mut ViewState, dx: f64, dy: f64);                             // screen px; content follows the fingers
  pub fn zoom_at(view: &mut ViewState, factor: f64, screen: (f64, f64));          // page point under `screen` stays put
  pub fn zoom_to_fit(view: &mut ViewState, doc: &Document, page: NodeId);         // doc.page_content_bounds(page) + ViewState::fit(r, 1.0); None → zoom 1 centred on (0, 0)
  ```
  (`ViewState` = `figcraft_engine::view_state::ViewState` or its re-export.)
- `render_view`:
  - origin = `view.origin()` − `OVERSCAN / zoom` on both axes
  - scale = `zoom · sf · quality`
  - width = `ceil((vw + 2·OVERSCAN) · sf · quality)`, and likewise for height
  - `None` when `vw < 1` or `vh < 1`
- `placement`:
  - x = `(frame.origin.0 − now.origin().0) · now.zoom`
  - w = `frame.width / frame.scale · now.zoom`
  - y and h likewise
- `dots` ports `ui-egui/src/canvas/board_bar.rs:298-330`:
  - `DOT_STEP = 24.0`, `DOT_MIN_GAP = 14.0`, and `dot_spacing` verbatim
  - size = `(1.5·√zoom).clamp(1, 3) / zoom`
  - ink is white on a dark page (luma < 128), black otherwise
  - alpha is 0.22 on a dark page, 0.18 on a light one; the faded dots use `alpha · fade`

- [ ] **Step 1: Write the failing tests.** Build the view with `ViewState::default()`, then set `cx`, `cy`, `zoom`, `vw`, `vh`:
  - `render_view_adds_overscan`: cx=0, cy=0, zoom=1, 800×600, sf 2, quality 1 → `origin (-656, -556)`, `scale 2`, `2624×2224`.
  - `render_view_is_none_for_empty_viewport`: vw 0 → `None`.
  - `placement_identity`: placing that frame on the same view → `[-256, -256, 1312, 1112]`.
  - `placement_follows_pan`: `pan(&mut v, 10.0, 0.0)` → x = −246.
  - `placement_follows_zoom`: `zoom_at(&mut v, 2.0, (400.0, 300.0))` → `[-912, -812, 2624, 2224]`.
  - `zoom_at_clamps_and_ignores_non_finite`: factor 1e9 → zoom 256; factor NaN → zoom unchanged; all fields finite.
  - `dots_double_until_far_enough`: `dots(0.25, [255;3]).unwrap().step == 96.0`; `dots(1.0, ..).step == 24.0`.
  - `to_bgra_composites_and_swaps`: 1×1 pixmap, opaque red → `[0,0,255,255]`; fully transparent over page `[10,20,30]` → `[30,20,10,255]`.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board view::`
Expected: FAIL (missing items).

- [ ] **Step 3: Implement `view.rs`.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board view::`
Expected: 8 passed.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/view.rs
git commit -m "feat(canvas): view maths for rendering, re-projecting and the board dot grid"
```

### Task 4: Render worker

**Files:**
- Modify: `crates/app/Cargo.toml` (deps `sawit-board = { path = "../board" }`, `figcraft-render.workspace = true`, `figcraft-doc.workspace = true`, `figcraft-engine.workspace = true`, `figcraft-text.workspace = true`)
- Create: `crates/app/src/board/mod.rs` (module root; Task 5 fills it)
- Create: `crates/app/src/board/worker.rs`
- Modify: `crates/app/src/main.rs` (`mod board;`)

**Interfaces:**
- Consumes: `sawit_board::view::{to_bgra, page_rgb}`.
- Produces:
  ```rust
  pub struct Job { pub doc: Arc<Document>, pub page: NodeId, pub view: figcraft_render::View, pub opts: figcraft_render::Options, pub page_rgb: [u8; 3], pub key: u64 }
  pub struct Frame { pub view: figcraft_render::View, pub image: Arc<RenderImage>, pub key: u64, pub millis: f64 }
  pub struct Worker;
  impl Worker {
      pub fn spawn() -> (Worker, futures::channel::mpsc::UnboundedReceiver<Frame>); // thread "canvas-render"
      pub fn request(&self, job: Job);   // latest wins: a pending job is replaced, never queued
  }
  ```
- The thread owns `figcraft_render::Retained`. For each job it does:
  1. `retained.render(..)`
  2. converts non-sRGB documents as `ui-egui/src/canvas/render_worker.rs:82 screen_image` does
  3. `to_bgra`
  4. wraps the result as `RenderImage::new([image::Frame::new(RgbaImage::from_raw(w, h, bgra))])`, as
     `viewers/pdf.rs:172` does
  5. logs `log::debug!("canvas render {millis:.1} ms")` and sends the frame
- The thread ends when `Worker` drops.

- [ ] **Step 1: Write the failing tests** (`worker.rs` tests; plain `#[test]`, `futures::executor::block_on` with a 5 s timeout loop):
  - `renders_a_job`: a board with one sticky, `View { origin: (-100,-100), scale: 1, width: 400, height: 300 }` → a `Frame` whose image size is 400×300 and whose key matches.
  - `latest_job_wins`: request keys 1, 2, 3 back to back → the last frame received has key 3, and the frame count is ≤ 3.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit board::worker`
Expected: FAIL (missing module).

- [ ] **Step 3: Implement `worker.rs`.** Use `Mutex<Option<Job>>` + `Condvar` for the latest-wins slot.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit board::worker`
Expected: 2 passed.

- [ ] **Step 5: Commit**
```bash
git add crates/app/Cargo.toml Cargo.lock crates/app/src/main.rs crates/app/src/board
git commit -m "feat(canvas): render figcraft pages on a background thread into gpui images"
```

### Task 5: `BoardView`: paint, pan/zoom, autosave, fonts

**Files:**
- Modify: `crates/app/src/board/mod.rs`
- Create: `crates/app/src/board/paint.rs` (frame + background painting)
- Modify: `crates/app/src/workspace.rs` (call `crate::board::init(cx)` from `workspace::init`)

**Interfaces:**
- Consumes: `store::{open, Writer, is_empty}`, `view::*`, `Worker`, `Job`, `Frame`.
- Produces:
  ```rust
  pub fn init(cx: &mut App);   // once: cx.background_spawn(figcraft_text::scan_system_fonts), then cx.refresh_windows()
  pub struct BoardView;        // impl Render, Focusable, EventEmitter<crate::canvas::ToggleCanvasMaximize>
  impl BoardView {
      pub fn open(path: PathBuf, cx: &mut Context<Self>) -> Self;
      pub fn path(&self) -> &Path;
      pub fn board(&self) -> Option<&BoardDoc>;    // None when the file failed to load
      pub fn board_mut(&mut self) -> Option<&mut BoardDoc>;
      pub fn flush(&mut self);                     // queue the current doc if it changed, then Writer::flush
  }
  ```
- Behaviour:
  - **Load:**
    - `open` loads with `store::open`.
    - On error the view renders `crate::states::Placeholder::error("The canvas couldn't be opened.")`
      with `.detail(err)`. It never saves, and it keeps the file.
    - The first frame calls `zoom_to_fit`.
  - **Each frame (in a `canvas()` prepaint, using `db/diagram.rs:797` as the model):**
    1. `session.set_ui_dark(cx.theme().mode.is_dark())` and `session.apply_view_prefs()`.
    2. Write `ds.view.vw` and `ds.view.vh` from the bounds.
    3. Compute `render_view` with `PREVIEW_QUALITY` while `zooming_until > now`, else 1.0.
    4. Build `key` = hash of (`Arc::as_ptr(doc)`, page, view fields, `figcraft_text::layout_generation()`,
       dark). When the key differs from the last request, call `worker.request(job)`.
    5. Build `opts` as `Options { lod: true, magnified_image_pixels: true, background: Some(page colour),
       dots: dots(zoom, page_rgb) when kind == Board, ..Default::default() }`.
  - **Paint:** fill the bounds with `page_rgb`, then `paint_image` the latest `Frame` at `placement(..)`,
    offset by the bounds origin.
  - **Frames:** a frame arriving from the receiver (a `cx.spawn` loop) replaces the current one and
    `cx.notify()`s. The replaced image is pushed to `stale` and released with `window.drop_image` in the
    next paint.
  - **Input:**
    - `on_scroll` (pattern `db/diagram.rs:372`) pans. With ⌘ held it zooms instead, at the cursor, by
      `exp(-dy · 0.01)`, and sets `zooming_until = now + 150 ms`.
    - `on_pinch` zooms at the cursor and sets the same deadline.
    - After a zoom, schedule a `cx.notify()` at the deadline so the full-quality render runs.
  - **Autosave:**
    - After any frame where `doc` is a different `Arc` from the last saved one, start a 300 ms debounce
      task.
    - The task queues `writer.save(path, to_bytes, is_empty(doc))`.
    - `Drop` calls `flush()`. So does a `cx.on_app_quit` hook registered in `open`, because ⌘Q does
      not drop entities.
    - A save error shows a toast: a small bottom-centre label in the canvas for 3 s, with the text from
      `writer.last_error()`.

- [ ] **Step 1: Write the failing tests** (`#[gpui_kit::test]`, pattern from `terminal/view.rs` tests):
  - `missing_canvas_opens_as_empty_board`: `cx.new(|cx| BoardView::open(tmp.join(".nyawit/canvas.figcraft"), cx))` → `board().unwrap().kind() == DocKind::Board`; the file does not exist.
  - `corrupt_file_shows_error_and_leaves_file`: write garbage, open → `board().is_none()`; after `flush()` the bytes are unchanged.
  - `edits_are_saved_after_flush`: open a missing path; run `board_mut().execute("board.sticky", ..)` through `update`; `flush()`; `store::open(path)` has one layer.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit board::tests`
Expected: FAIL (missing `BoardView`).

- [ ] **Step 3: Implement `BoardView`, `paint.rs` and `init`.**

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit board::`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/app/src/board crates/app/src/workspace.rs
git commit -m "feat(canvas): native canvas view with pan, zoom, dot grid and autosave"
```

### Task 6: Workspace wiring behind the flag, plus `get` and `command` ops

**Files:**
- Create: `crates/board/src/ops.rs`
- Modify: `crates/app/src/workspace.rs`:
  - field `native_canvas: bool`, set from `std::env::var_os("SAWIT_NATIVE_CANVAS").is_some()` in
    `Workspace::new`
  - field `boards: Vec<WeakEntity<BoardView>>`
  - `open_canvas` returns `CanvasTab` (below)
- Modify: `crates/app/src/workspace/control.rs` (`Command::Canvas` answers `Answer::Now(board.run_op(op))` when native)
- Modify: `crates/app/src/board/mod.rs` (`pub fn run_op(&mut self, op: &Value, cx: &mut Context<Self>) -> Value`)

**Interfaces:**
- Produces:
  ```rust
  // sawit-board
  pub fn run(board: &mut BoardDoc, op: &Value) -> Value; // op "get" | "command"; other ops → {ok:false, error:"<op> is not supported yet"}
  // get → {ok:true, elements:[{id, type:"other", name, x, y, width, height}]} for the page's top-level layers
  //       (Plan 7 replaces this with the full Summary)
  // command {command, params?} → {ok:true, result} | {ok:false, error}
  // app
  pub enum CanvasTab { Web(Entity<CanvasView>), Native(Entity<BoardView>) }
  ```
- `open_canvas`:
  - Keeps today's tab key (`canvas\0<root>` / `canvas`), title and icon.
  - With `native_canvas` it creates
    `BoardView::open(board_file(root.as_deref(), &AppPaths::default_macos().root), cx)`.
  - It subscribes to `ToggleCanvasMaximize` the same way the web canvas does, and pushes the view onto
    `boards`.
- Callers of `open_canvas`: `OpenCanvas`, `Page::Canvas`, `Command::Canvas`.

- [ ] **Step 1: Write the failing tests.**
  - `ops.rs`:
    - `get_lists_top_level_layers`: two stickies → 2 elements, each with a numeric-string `id`.
    - `command_runs_engine_commands`: `{op:"command", command:"board.sticky", params:{x:0,y:0}}` → `ok:true` and `result.id`.
    - `command_errors_are_reported`: unknown command → `ok:false` and an `error` that contains the id.
    - `unknown_op_is_reported`: `{op:"mermaid"}` → `ok:false`.
  - `workspace.rs`: `native_flag_opens_board_view`. Set `workspace.native_canvas = true` in a GPUI test,
    call `open_canvas`, and expect `CanvasTab::Native(_)` with the active tab key starting `"canvas"`.
    Write it only if `Workspace` can be built in a test the way the existing workspace tests build it.
    Otherwise put this check in Task 7's live verification.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit-board ops:: && cargo test -p sawit workspace`
Expected: FAIL.

- [ ] **Step 3: Implement** `ops.rs`, `run_op`, the flag, `CanvasTab` and the control branch.

- [ ] **Step 4: Run them**
Run: `cargo test -p sawit-board && cargo test -p sawit`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/board/src/ops.rs crates/board/src/lib.rs crates/app/src/workspace.rs crates/app/src/workspace/control.rs crates/app/src/board/mod.rs
git commit -m "feat(canvas): open the native canvas with SAWIT_NATIVE_CANVAS and answer get and command"
```

### Task 7: Live verification

- [ ] **Step 1: Build and launch** (memory recipe "Visual verify Sawit"):
```bash
cargo build -p sawit
S=$(mktemp -d /tmp/sawit-nc-XXXX); mkdir -p $S/home $S/proj/.nyawit
cp $(ls -d ~/.cargo/git/checkouts/figcraft-*/0971d09*/examples | head -1)/ridgeline-app.figcraft $S/proj/.nyawit/canvas.figcraft
SAWIT_HOME=$S/home SAWIT_NATIVE_CANVAS=1 RUST_LOG=sawit=debug target/debug/sawit $S/proj &
```

- [ ] **Step 2: Open the canvas through the control socket.**
  - Send `{"cmd":"page","page":"canvas"}`. Find the socket with `{"cmd":"state"}`, and only for this
    instance's socket.
  - Then send `{"cmd":"canvas","op":{"op":"get"}}`.
  - Expected: `ok:true`, with non-empty elements.

- [ ] **Step 3: Screenshot the window** (`screencapture -x -o -l <id>`).
  - Expected: the example document is visible and zoomed to fit.
  - Then send `{"cmd":"canvas","op":{"op":"command","command":"view.zoomIn"}}` and screenshot again.
    Expected: zoomed in.

- [ ] **Step 4: Check the render times.**
  - Read the `canvas render … ms` debug lines.
  - Record the p50 and p95 for the fit view in this plan's notes. The Plan 8 budget compares against
    them.

- [ ] **Step 5: Stop only this instance** with `/bin/kill <pid>` (`pgrep -f "$S/home"`), then run the full gates:
Run: `cargo test --workspace`
Expected: all pass.

#### Task 7 notes: live check (2026-10-30, at `976c41d`, lane 4)

All checks passed, and `cargo test --workspace` passes: 852 passed, 9 ignored, 0 failed. Machine:
Apple M3 (8 cores, 16 GB), macOS 26.6.1, window scale factor 2. Every number below comes from the
debug build **under build load** (R-LOAD). Other lanes were compiling, and `vm.loadavg` was 2.7–4.6
during the runs (per run in the table).

**Launch recipe.** When a later plan says "launch as in Plan 1 Task 7", it means this recipe. It
replaces Steps 1, 2 and 5 above, as Plan 1 rulings C4, C6 and C7 amend them:

```bash
cargo build -p sawit                       # in the checkout or lane under test
S=<scratch dir>/live-<task>; mkdir -p "$S/home" "$S/proj"
# The example document. Skip this line for an empty board: the first save then creates .nyawit/ and its .gitignore.
mkdir -p "$S/proj/.nyawit" && cp ~/.cargo/git/checkouts/figcraft-bc55fa7002b5fc9c/0971d09/examples/ridgeline-app.figcraft "$S/proj/.nyawit/canvas.figcraft"
SAWIT_HOME=$S/home SAWIT_NATIVE_CANVAS=1 RUST_LOG=sawit=debug target/debug/sawit --foreground "$S/proj" 2>"$S/sawit.log" &
PID=$!
# Once the app is up: $S/home/sawit.sock, or /tmp/sawit-$USER-<hash>.sock when that path is 100 bytes or more.
SOCK=$(lsof -p $PID -a -U | grep -o '/[^ ]*\.sock$')
ctl() { python3 -I -c 'import socket,sys; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); s.sendall(b"sawit-control/1\n"+sys.argv[2].encode()); s.shutdown(socket.SHUT_WR); print(s.makefile().read(), end="")' "$SOCK" "$1"; }
ctl "{\"cmd\":\"open_page\",\"page\":\"canvas\",\"project\":\"$S/proj\"}"
ctl '{"cmd":"canvas","op":{"op":"get"}}'    # each reply is one line: "ok <json>" or "error: <message>"
/bin/kill $PID                              # this PID only: `kill` is aliased, and a broad pgrep finds the user's app
```

- `--foreground` keeps the app in this process. `$PID` is then the window's process, and stderr
  (the `canvas render … ms` lines) goes to the log. Without the flag, the binary starts itself
  again with stderr sent to `/dev/null`.
- Screenshots: list `$PID`'s windows with `CGWindowListCopyWindowInfo` (a few lines of Swift), and
  take the 1280×801 one with `screencapture -x -o -l <window id>`. A covered window doesn't
  repaint, and only `{"cmd":"focus"}` makes the control server raise the window.
- A `get` that answers `type: "other"` with numeric-string ids is how a check tells the native
  canvas from the web one (C8).
- Drop `SAWIT_NATIVE_CANVAS=1` once Plan 8 Task 1 has removed the flag.

**Answers (Steps 2–3, C8, and the checks carried over from Task 6).**

- Build: `cargo build -p sawit` succeeds. Its only linker line is the known R-EHFRAME warning.
- (a) `open_page canvas` answers `{"tab":0}`. `state` then lists exactly one tab: kind `canvas`,
  active, with `canvas_open: true`.
- C8: `get` answers `ok:true` with the page's 4 top-level layers (Components, Home — Light,
  Trail — Light and Home — Dark). All are `type: "other"`, with ids `"349"`, `"68"`, `"239"` and
  `"178"`: this is the native canvas.
- (b) A second `open_page canvas` leaves the tab count (1) and the active index (0) unchanged.
  `get` answers the same ids.
- Step 3: the screenshot shows the example zoomed to fit on a 1040×471 pt canvas. The zoom is
  0.3073, which is (471 − 2·64)/1116.3, set by the content's height. The view is centred on the
  content at (19.5, 558.15). `view.zoomIn` answers `{"zoom":0.5}`, and the next screenshot shows
  the canvas zoomed in about the middle of the view.
- (c) `text.create {x:0, y:0, text:"p1t7-autosave"}` answers `ok:true` with `result.id` 350. The
  next `get` lists `"350"`. The file's `document.json` holds the text 0.47 s after the op.
- Opening the file wrote nothing. More than 120 view commands left it byte-identical, because view
  changes aren't edits.
- Also checked over the socket (beyond the steps above):
  - **Empty board.** A project with no canvas file (a second project in the window) opens an
    empty board with the dot grid. `get` answers `[]`, and nothing is written. The first
    `board.sticky` answers `{"id":2,"text":3}`. 0.42 s later `.nyawit/` exists, with its
    `.gitignore` and the canvas.
  - **Closing the tab saves.** `text.create`, then `close_tab` at once. The text was in the file
    17 ms after the op, before the 300 ms autosave could run. `canvas_open` turned false (the view
    was released), and reopening listed the text again.
  - **A failed save.** With `.nyawit/` read-only, `text.create` shows the toast "The canvas
    couldn't be saved to …/canvas.figcraft: Permission denied (os error 13)". The file stays as it
    was, and the document keeps the text. Once the folder was writable again, closing the tab wrote
    the text.
  - **A truncated file.** With `canvas.figcraft` cut to its first 20,000 bytes, the tab shows "The
    canvas couldn't be opened." with "not a valid FigCraft file: invalid Zip archive: Could not
    find EOCD". `get` and `command` answer `ok:false` with the path and that reason. The file is
    byte-identical after an attempted edit and after closing the tab, and nothing is written beside
    it.
  - **The log.** All 130 lines of the debug log are `canvas render` lines: one for each render that
    an opened canvas, a resize, a view change or an edit called for. There are no warnings or
    errors.

**Render times (Step 4): debug build, under build load.** Each sample is the one
`canvas render … ms` line caused by one control command; the commands went one at a time.
Percentiles are nearest-rank. Each frame size is the canvas plus 256 pt of overscan on every side,
at 2×.

| Run | Canvas (pt) | Frame (device px) | Renders | p50 (ms) | p95 (ms) | min–max (ms) | `vm.loadavg` before → after |
|---|---|---|---|---|---|---|---|
| First render after launch (cold) | 1040×471 | ≈3105×1967 | 1 | 395.2 | – | – | { 2.70 3.15 3.04 } |
| **Fit view**: `view.zoomFit` after each `view.zoomIn` | 1040×471 | ≈3105×1967 (6.1 Mpx) | 20 | **147.4** | **177.6** | 122.9–183.9 | { 2.90 2.93 2.96 } → { 3.63 3.10 3.02 } |
| `view.zoomIn`/`view.zoomOut` cycles from fit (Plan 8's method) | 1040×471 | 6.1 Mpx | 20 | 149.9 | 188.4 | 105.8–192.8 | { 3.67 3.13 3.03 } → { 4.57 3.32 3.10 } |
| **Fit view**, with the sidebar and terminal hidden | 1280×711 | ≈3585×2447 (8.8 Mpx) | 20 | **179.2** | **183.9** | 165.6–185.0 | { 4.48 3.42 3.14 } → { 4.04 3.39 3.13 } |
| Zoom cycles from fit, with the sidebar and terminal hidden | 1280×711 | 8.8 Mpx | 20 | 164.7 | 186.7 | 141.9–187.4 | { 4.04 3.39 3.13 } → { 3.57 3.31 3.11 } |

- Single edits, with the sidebar and terminal hidden: `text.create` took 146.0 ms and
  `board.sticky` 135.9 ms.
- What a line times: `Retained::render`, whose FigCraft crates are at opt-level 3 since Task 5,
  plus `sawit_board::view::to_bgra` and the image wrap. The dev profile builds those two at
  opt-level 0.
  - A verbatim copy of `to_bgra`, timed alone on these frame sizes, takes 84 ms (6.1 Mpx) and
    119 ms (8.8 Mpx) at opt-level 0 with debug assertions, and 1.5 ms and 2.2 ms at opt-level 3.
  - So most of each debug line is that conversion. That is why a small edit costs about as much as
    a zoom here.
  - These numbers are only indicative. Plan 8 Task 4 measures the budget on the release build,
    where the conversion takes about 2 ms.
- Idle: with the design canvas showing and no input, `top`'s 1 s samples of the app read 0.5–1.6 %
  CPU over 10 s. No render line appeared, so nothing redraws continuously.
- Observations for later plans:
  - A save writes `document.json` only, so the example's `thumbnail.png` isn't kept (58,167 →
    28,664 bytes). The thumbnail is FigCraft's recent-files preview, which it reads as optional
    (figcraft-format `thumbnail.rs`), so no content is lost. It matters to Plan 7's in-place saves
    of third-party `.figcraft` files (R-D14).
  - After a failed save, nothing retries until the next edit or flush (closing the tab, quitting).
    This is what `BoardView::flush` documents.

#### Hand checks (pending — needs the user)

Launch with the recipe above, and run its `open_page` line.

1. **Autosave while the window is covered or minimised.** This comes from Task 6's review: an op
   sent while the window's display link is stopped.
   - Minimise the Sawit window (⌘M), or cover it completely with another app's window.
   - Run `ctl '{"cmd":"canvas","op":{"op":"command","command":"text.create","params":{"x":0,"y":0,"text":"covered-save"}}}'`.
     Expect `ok:true`.
   - Wait 2 s with the window still minimised or covered. Then run
     `unzip -p "$S/proj/.nyawit/canvas.figcraft" document.json | grep -c covered-save`. Expect 1
     or more.
   - Bring the window back. Expect no crash and no blank canvas: the canvas redraws at once.
   - Run `ctl '{"cmd":"canvas","op":{"op":"command","command":"view.zoomSelection"}}'`. Expect the
     new text to show.
2. **Panning with the trackpad.** Scroll with two fingers over the canvas.
   - The content follows the fingers and moves at once. The last frame moves with them until the
     new render arrives.
   - A fast pan of more than about 256 pt shows the page colour at the leading edge for a moment,
     then the content fills in.
   - Nothing jumps or flickers when a new frame replaces the moved one.
   - A debug render takes 0.1–0.2 s. To judge smoothness, also try a release build: run
     `cargo build --release -p sawit`, then the same launch with `target/release/sawit`.
3. **Zooming with gestures.** Pinch, and scroll with two fingers while holding ⌘.
   - Both zoom about the pointer: the spot under the pointer stays under it.
   - During the gesture the image is a little soft, because it renders at half resolution. About
     0.15 s after the fingers stop, it is sharp again.
   - At rest, text and 1 px lines are crisp, with no half-pixel blur.
4. **Opening the canvas by hand.** Put the canvas tab in the background, press ⌃⌘C, and then click
   the title bar's Canvas button. Expect each to bring the same canvas tab forward, with no second
   canvas tab.
5. **Quitting right after an edit (needs Plan 2's editing).** For now, an edit made over the socket
   is saved 0.3 s later, which is too soon to press ⌘Q first.
   - Once Plan 2 lands, move a layer with the mouse and press ⌘Q at once.
   - Relaunch and open the canvas. Expect the move to be kept, which shows that the quit hook saved
     it.
   - Closing the tab uses the same save, and that was checked live above.
