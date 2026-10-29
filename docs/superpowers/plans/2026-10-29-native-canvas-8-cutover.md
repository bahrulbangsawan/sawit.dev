# Native canvas 8: Cutover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The native canvas becomes the only canvas:
- the flag is gone
- Excalidraw, the WKWebView (`wry`), the embedded web bundle and `web/canvas/` are deleted
- MCP descriptions, README and the consolidate command describe the native canvas
- FigCraft is credited in `NOTICE`
- the performance budget is measured and recorded

**Architecture:** This is a removal plan:
- `open_canvas` always builds `BoardView`.
- Code that existed only for the webview goes: syncing visibility, passthrough keys, deferred
  `Answer::Later` replies, and brotli bundle serving.
- `nyawit::canvas_file` is renamed to `legacy_canvas_file` and kept only for migration and the MCP
  fallback.

**Tech Stack:** As Plan 1.

**Spec:** `docs/superpowers/specs/2026-10-29-native-canvas-design.md` (D6, D16, "Performance budget", "Risks")

## Global Constraints

- Toolchain: Rust 1.99.1 (`rust-toolchain.toml`), edition 2024, workspace member crates only.
- FigCraft deps are pinned in root `[workspace.dependencies]` to rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`.
- `crates/board` never depends on GPUI.
- User-facing name stays "Canvas". The ⌃⌘C shortcut, the title-bar button, `app_open_page canvas` and the `canvas_*` tool names don't change.
- Gates before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the touched crates' tests.
- Commits:
  - Commit on `main` with explicit `git add <paths>` (deletions with `git rm`).
  - Conventional Commits with lowercase subjects. Every message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - **Never `git push`.** The user releases with `/consolidate`.

## Review Focus

1. A user upgrading with only `.nyawit/canvas.excalidraw` must see their board on first open, and the file must stay. Test: `migration_imports_once_and_keeps_original` (Plan 7). It is re-run in Task 4's live check against a real old board.
2. Old control clients sending `{"cmd":"canvas","op":{"op":"get"}}` must get an immediate answer, not a timeout. Test: `canvas_ops_answer_now` (Task 1).
3. No dangling reference to the webview may remain: no `wry`, `include_dir`, `brotli` or `CanvasView`. Test: `rg` check in Task 1 Step 4 and a clean `cargo build`.
4. ⇧⌘↩ must still maximize the canvas tab, and the tab bar must come back after. Test: `maximize_still_works_without_webview` (Task 1).
5. The release binary must build with LTO, and its size change must be reported. Test: Task 4 Step 2.

---

### Task 1: Remove the flag and the web canvas

**Files:**
- Delete: `crates/app/src/canvas.rs`, `crates/app/canvas/` (all `.br` files), `web/canvas/`
- Modify: `crates/app/src/board/mod.rs` (move `TITLE`, `ICON` and `ToggleCanvasMaximize` here from `canvas.rs`)
- Modify: `crates/app/src/main.rs` (drop `mod canvas;`)
- Modify: `crates/app/src/workspace.rs`:
  - drop `native_canvas`, `CanvasTab::Web`, `canvases`, `sync_canvas`, `canvas_passthrough_keys`, its
    test `the_canvas_passes_on_app_shortcuts_but_not_its_own`, and `page_key` if it is now unused
  - `open_canvas` returns `Option<Entity<BoardView>>`
  - update the "canvas" comment at the old :2014
- Modify: `crates/app/src/workspace/control.rs` (`Command::Canvas` → `Answer::Now`; delete `Answer::Later` if nothing else uses it)
- Modify: `crates/app/src/cli.rs`:
  - drop the canvas-only 30 s `CONTROL_TIMEOUT` branch and the `Answer::Later` handling, if unused
  - keep the 64 MB read limit; reword its comment to "canvas scenes and images can be large"
- Modify: `crates/app/Cargo.toml` (remove `wry`, `include_dir`, `brotli-decompressor`, and their comments)
- Modify: `crates/core/src/nyawit.rs` (`canvas_file` → `legacy_canvas_file`; update the callers in `crates/board/src/store.rs` and `crates/mcp/src/app.rs`)

**Interfaces:**
- Produces: `crate::board::{TITLE, ICON, ToggleCanvasMaximize}`. Every former `crate::canvas::` path points here.

- [ ] **Step 1: Write the failing tests** (`#[gpui_kit::test]` in `workspace.rs`; if a `Workspace` can't be built in a test, check these in Task 4 instead):
  - `canvas_opens_native_board`: `open_canvas` with no env var → a `BoardView` tab keyed `canvas…`.
  - `canvas_ops_answer_now`: `Command::Canvas {op:{"op":"get"}}` → `Answer::Now` with `ok:true`.
  - `maximize_still_works_without_webview`: `toggle_canvas_maximized` → layout `CENTER_ONLY` and the tab bar hidden; a second call → the previous layout.

- [ ] **Step 2: Run them**
Run: `cargo test -p sawit workspace`
Expected: FAIL (the flag is still required).

- [ ] **Step 3: Delete and rewire** as listed. Prefer `git rm -r crates/app/canvas web/canvas crates/app/src/canvas.rs`.

- [ ] **Step 4: Check for leftovers**
Run: `rg -n "wry|include_dir|brotli|CanvasView|canvas_passthrough|Excalidraw|excalidraw" crates/ --glob '!crates/board/src/excalidraw.rs' --glob '!crates/board/tests/**'`
Expected: matches only in import, migration and MCP fallback code (`legacy_canvas_file`, `excalidraw::`), and in the `canvas_load` description.

- [ ] **Step 5: Run the gates**
Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass.

- [ ] **Step 6: Commit**
```bash
git add -u crates web Cargo.toml Cargo.lock
git commit -m "feat(canvas): replace the excalidraw webview with the native figcraft canvas"
```

### Task 2: MCP wording

**Files:**
- Modify: `crates/mcp/src/app.rs`:
  - `CanvasElementsArgs` description: "Element skeletons" (types rectangle, ellipse, diamond, sticky, text, arrow, line, frame)
  - `CanvasLoadArgs`: "A .figcraft file to load, or an .excalidraw file to import (replaces the board)"
  - `PAGES` / `app_open_page`: "canvas (whiteboard and design canvas)"
- Modify: `crates/mcp/src/lib.rs` (server instructions at the old :84 and :86: "canvas_*: the canvas (FigCraft engine): …")

- [ ] **Step 1: Write the failing test** (`lib.rs`): `canvas_tools_describe_the_native_canvas`. No tool description or instruction line contains "Excalidraw", except `canvas_load`'s mention of importing `.excalidraw`.

- [ ] **Step 2: Run it**
Run: `cargo test -p sawit-mcp`
Expected: FAIL.

- [ ] **Step 3: Update the strings.**

- [ ] **Step 4: Run it**
Run: `cargo test -p sawit-mcp`
Expected: all pass.

- [ ] **Step 5: Commit**
```bash
git add crates/mcp/src
git commit -m "docs(mcp): describe the native canvas in the canvas tools"
```

### Task 3: README, NOTICE, consolidate

**Files:**
- Create: `NOTICE`:
  - "Sawit includes FigCraft (https://github.com/storytold/figcraft), Copyright (c) 2026 ArtCraft Team and the FigCraft contributors, licensed under MIT OR Apache-2.0."
  - The text of FigCraft's `NOTICE` file, copied verbatim from the pinned checkout `~/.cargo/git/checkouts/figcraft-*/0971d09*/NOTICE`.
- Modify: `README.md`:
  - The `### Canvas` section covers:
    - boards and design files
    - tools and the toolbar
    - layers and Design panels
    - menus, the Actions palette (⌘K) and shortcuts (⌃⇧?)
    - where files live (`.nyawit/canvas.figcraft`, exports in `.nyawit/exports/`)
    - migration from `.excalidraw`
  - The MCP table row lists `canvas_command` and `canvas_commands`.
  - The demo GIF caption no longer says Excalidraw.
  - Add a Credits line for FigCraft.
- Modify: `.claude/commands/consolidate.md` (remove the `web/canvas` → `npm run build` rule)

- [ ] **Step 1: Check the wording**
Run: `rg -n "Excalidraw|excalidraw" README.md .claude/commands/consolidate.md`
Expected: only the migration sentence in README.

- [ ] **Step 2: Commit**
```bash
git add NOTICE README.md .claude/commands/consolidate.md
git commit -m "docs: the canvas runs on the figcraft engine; credit figcraft"
```
`docs/media/canvas.png` and the demo video are refreshed by the user's `/consolidate` run (phase 5), not here.

### Task 4: Budget, size and the final check

- [ ] **Step 1: Release build**
Run: `cargo build --release -p sawit`
Expected: success.

- [ ] **Step 2: Size**
Run: `ls -l target/release/sawit`
Record the size next to the baseline from Plan 1 Task 0 Step 3 in this plan.

- [ ] **Step 3: Budget run.**
  - Launch the release build with a scratch `SAWIT_HOME` and `RUST_LOG=sawit=debug`.
  - Build a board of about 500 layers through `canvas_command`: `board.template` ×10, then `edit.duplicate` until `get` reports ≥ 500.
  - Zoom to fit.
  - Record the p50/p95 of `canvas render … ms` over 20 renders after `view.zoomIn`/`view.zoomOut` cycles.
  - Record idle CPU: `ps -o %cpu= -p <pid>`, sampled 10 times, 1 s apart, with no input.
  - Expected: p95 ≤ 50 ms and idle CPU ≈ 0 %. If it misses, record the numbers and open a follow-up note in the spec's Risks. Do not tune here.

- [ ] **Step 4: Migration on a real old board.**
  - Copy a real `.nyawit/canvas.excalidraw` (ask the user for one) into a scratch project and open the canvas.
  - Expected: the screenshot shows the content, and the `.excalidraw` checksum is unchanged.

- [ ] **Step 5: Hand-check with the user.** Walk through each item and record the answers here:
  - ⌃⌘C opens the canvas; ⇧⌘↩ maximizes
  - tools, flyouts, stickies, connectors, text with IME
  - layers drag and rename
  - Design panel fields
  - menus, the palette and the sheet
  - copy/paste between two project canvases
  - export
  - `canvas_add_mermaid` from Claude Code

- [ ] **Step 6: Final gates**
Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass. Tell the user it is ready for `/consolidate` (release). Do not push.
