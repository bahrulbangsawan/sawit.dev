# Native canvas 8: Cutover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The native canvas becomes the only canvas:
- the flag is gone
- Excalidraw, its WKWebView, the embedded web bundle and `web/canvas/` are deleted; `wry` stays for
  the browser tab (ruling P8-C1)
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
3. No dangling reference to the canvas's webview may remain: no `include_dir`, `brotli` or `CanvasView`, and `wry` only in the browser (`crates/app/src/browser/`, ruling P8-C1). Test: `rg` check in Task 1 Step 4 and a clean `cargo build`.
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
- Modify: `crates/app/Cargo.toml` (remove `include_dir`, `brotli-decompressor` and the canvas-bundle comment; `wry`, `objc2-web-kit`, `objc2-app-kit` and `block2` stay for the browser tab in `crates/app/src/browser/`, ruling P8-C1)
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
Run: `rg -n "wry|include_dir|brotli|CanvasView|canvas_passthrough|web/canvas|pele\.ts|Excalidraw|excalidraw" crates/ --glob '!crates/board/src/excalidraw.rs' --glob '!crates/board/tests/**' --glob '!crates/app/src/browser.rs' --glob '!crates/app/src/browser/**'` (rulings P8-C1, P8-C6)
Expected: matches only in import, migration and MCP fallback code (`legacy_canvas_file`, `excalidraw::`), in the MCP tool and argument descriptions (Task 2's test is their gate), in the `ensure()` test fixture of `crates/core/src/nyawit.rs`, and in `crates/app/Cargo.toml`'s `wry` line and `objc2-web-kit` comment.

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

Rulings amend these steps, as written below: P8-C8 (Step 3's socket checks, Step 5's maximize
item), P8-C12 (Step 3's launch, the stop before Step 6), P8-C13 (Step 3's board and canvas size) and
P8-C14 (Step 7). The carried checks of the controller's P8.T4 checklist (Plan 8 ledger) are in
"Task 4 notes" below.

- [ ] **Step 1: Release build**
Run: `cargo build --release -p sawit`
Expected: success.

- [ ] **Step 2: Size**
Run: `ls -l target/release/sawit`
Record the size next to the baseline from Plan 1 Task 0 Step 3 in this plan.

- [ ] **Step 3: Budget run.**
  - Launch the release build with a scratch `SAWIT_HOME` and `RUST_LOG=sawit=debug`:
    `SAWIT_HOME=$S/home RUST_LOG=sawit=debug target/release/sawit --foreground "$S/proj" 2>"$S/render.log" &`,
    then `PID=$!`. Bring the window forward with `{"cmd":"open_page","page":"canvas","project":"$S/proj"}`
    (ruling P8-C12).
  - With no `SAWIT_NATIVE_CANVAS`: `state` shows an active tab of kind `canvas`, and
    `{"cmd":"canvas","op":{"op":"get"}}` answers `ok:true` at once, well under the old 30 s (ruling P8-C8).
  - Build a board of about 500 layers through `canvas_command`: `board.templates`, then
    `board.template {name}` ×10, cycling through its ids, then `select.all` and `edit.duplicate` until
    `node.find {limit: 0}` reports a total ≥ 500. Record that total and `get`'s top-level count
    (ruling P8-C13).
  - Hide the sidebar and terminal with the `layout` command, and record the canvas's size in device
    pixels (ruling P8-C13).
  - Zoom to fit.
  - Record the p50/p95 of `canvas render … ms` over 20 renders after `view.zoomIn`/`view.zoomOut` cycles.
  - Record idle CPU: `ps -o %cpu= -p <pid>`, sampled 10 times, 1 s apart, with no input.
  - Expected: p95 ≤ 50 ms and idle CPU ≈ 0 %. If it misses, record the numbers and open a follow-up note in the spec's Risks. Do not tune here.

- [ ] **Step 4: Migration on a real old board.**
  - Copy a real `.nyawit/canvas.excalidraw` into a scratch project and open the canvas. Use a copy,
    never the original: the old board this Mac has (`~/Library/Application Support/sawit/canvas.excalidraw`),
    else Plan 7 Task 4's fixture (cross-plan ruling).
  - Expected: the screenshot shows the content, and the `.excalidraw` checksum is unchanged.

- [ ] **Step 5: Hand-check with the user.** Walk through each item and record the answers here:
  - ⌃⌘C opens the canvas; ⇧⌘↩ maximizes, and ⌘B or switching tabs brings the tab bar back
    (ruling P8-C8)
  - panning the budget board keeps up (ruling P8-C13)
  - bold Inter on a Mac without Inter (the P2.T7 carry)
  - a real old board: which import losses matter (the P7.T4 carry)
  - tools, flyouts, stickies, connectors, text with IME
  - layers drag and rename
  - Design panel fields
  - menus, the palette and the sheet
  - copy/paste between two project canvases
  - export
  - `canvas_add_mermaid` from Claude Code

- [ ] **Step 6: Final gates.** First stop the instance with `/bin/kill $PID` (ruling P8-C12).
Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass. Tell the user it is ready for `/consolidate` (release). Do not push.

- [ ] **Step 7: Commit the notes** (ruling P8-C14)
```bash
git add docs/superpowers/plans/2026-10-29-native-canvas-8-cutover.md docs/superpowers/specs/2026-10-29-native-canvas-design.md
git commit -m "docs: record the native canvas budget, release size and final checks"
```
The spec is added only when a Risks note was written.

#### Task 4 notes: final check (2026-10-31, at `e52cf5f`, lane 5)

Every unattended step ran. The release build, the socket checks, the real board's migration, edits
and idle CPU pass. The settle-render budget misses on the budget board: p95 537 ms, and 382 ms at
500 nodes, against 50 ms. The stickies' drop shadows cost most of it. Both boards are piles, with
every template at the view's centre, so these are upper bounds: laid out over their page, the same
501 nodes settle inside the budget, at p95 36–47 ms (fix round 1). As Step 3 says, nothing was
tuned: the follow-up is in the spec's Risks, beside the save-cost one the P7.T6 ruling asks for.
Step 6's gates pass. What needs a person is under "Hand checks".

How it ran:
- Machine: Apple M3 (8 cores), 16 GB, macOS 26.6.1, window scale factor 2.
- The release build of `e52cf5f` (main), launched from the lane as P8-C12 says, with no
  `SAWIT_NATIVE_CANVAS`, and stopped with `/bin/kill $PID`. Three launches, load average at each
  launch in brackets:
  1. an empty project: the socket checks, the budget board and its measurements (2.64 2.96 3.47);
  2. a project holding a copy of the real old board: Step 4 (4.44 10.36 10.83);
  3. fix round 1, a fresh home and an empty project: the board laid out over its page (3.73 4.44
     6.34). The binary was the same build, kept in the scratchpad, with the same SHA-256.
- Everything went over the control socket. Screenshots came from window listing and
  `screencapture -l`. No AX, osascript or posted events. The window kept Sawit's 1280 × 801 and
  stayed unobstructed, so `focus` was never sent. No change was seen that no op of this check made
  (P3.T4 carry).
- Load (R-LOAD): the machine wasn't quiet. Free disk stayed near 8 GB, under the run's 10 GB
  watchdog line, so the watchdog re-cloned the lanes' `target/` folders every 5 minutes. Each pass
  put the 1-minute load at 20–50 for about 75 s, and macOS's storage services used one or two cores
  throughout. Every series ran between passes, with `top` showing 70–95 % of the CPU idle as it
  started. The load averages in the tables still carry the last pass. Repeats agree closely (the
  budget board's 50 % renders took 222–244 ms in three series), so the verdicts don't rest on the
  load. The third launch's series ran at a 1-minute load of 2.9–3.7, with 64–78 % of the CPU idle
  as each started.

Step 1: `cargo build --release -p sawit` succeeded in 10 min 13 s, from clean. Its one other line is
the future-incompatibility note for `block 0.1.6`, which the baseline's `Cargo.lock` already had.

Step 2:

| `target/release/sawit` | Bytes | MiB |
|---|---|---|
| Baseline (Plan 1 Task 0, `0f5df5e`) | 35,670,592 | 34.02 |
| Now (`e52cf5f`) | 60,367,760 | 57.57 |
| Change | +24,697,168 (+69.2 %) | +23.55 |

- The embedded Inter fonts add 1,789,960 bytes (1.71 MiB, 7 % of the change). `OFL-Inter.txt`
  isn't embedded.
- The Excalidraw bundle left with the webview: −1,550,289 bytes (1.48 MiB, 107 files).
- The rest, +24,457,497 bytes (23.32 MiB), is code: FigCraft's engine, renderer, text, format and
  document crates with their dependencies, `sawit-board`, the canvas views, and `sawit-mcp`'s
  headless canvas (the app binary is also `sawit-mcp`).
- All 97 commits since the baseline belong to this program, so nothing else is in the change.
  `wry` stays for the browser tab (P8-C1).
- The release `__eh_frame` is 3,733,104 bytes (3.56 MiB), up from 2.23 MiB.

Step 3, the checks before the budget run (P8-C8 and the P8.T1 delegate's carries):
- `open_page canvas` answered `{"tab":0}` (1.3 ms). `state` then listed one tab, of kind `canvas`,
  active.
- `{"cmd":"canvas","op":{"op":"get"}}` answered `ok:true` with no elements, five times, each round
  trip 0.0–0.1 ms as the client timed it. On the budget board it took 2.0 ms. Old clients get their
  answer at once (Review Focus 2).
- `text.fonts {family: "Inter"}` answered `ok:true` with 18 styles, Thin to Black with their
  italics, so Regular, Bold and Italic too. This Mac has no Inter installed, so the list comes from
  the fonts `board::init` registers.

The budget board (P8-C13):
- `board.templates` lists retrospective, brainstorm, weeklyPlanner, flowChart and kanban. Ten
  `board.template {name}`, cycling through them, placed 4, 13, 6, 12, 4, 4, 13, 6, 12 and 4
  top-level layers, centred in the view: 194 nodes. `select.all` and `edit.duplicate` made 388,
  then 776. The recipe doubles past 500, so a 501-node board was measured too (below).
- `node.find {limit: 0}`: 776. `get`: 312 top-level layers (88 sections, 80 connectors, 64
  stickies, 24 texts, 24 ellipses, 16 rounded rectangles, 8 rectangles, 8 diamonds). 192 layers
  carry an effect: the soft drop shadow FigCraft gives every sticky.
- The board is one pile. Given no `x` and `y`, `board.template` puts every template at the view's
  centre, and the duplicates land on or beside what they copy, so the 776 nodes lie in one heap
  about 1,950 × 800 pt. A 100 % render covers about 90 % of it, so it blurs nearly all 192 shadows
  again each time. The 501-node board is a pile too. Their settle times are upper bounds: a board
  laid out over its page puts fewer shadows, or smaller ones, in each render (below).
- With the sidebar and terminal hidden, the canvas is 1280 × 675 pt: 2560 × 1350 device px, where
  the spec's budget names 2560 × 1600. Each render covers the canvas and 256 pt more on every side:
  3584 × 2374 px, 8.5 Mpx. Zoom to fit: 59 %.

The board laid out over its page (fix round 1, the P8.T4 delegate's ruling). It supplements the
budget board, which stays the board of record with its numbers unchanged:
- The third launch, with one frame painted before any view op, and the sidebar and terminal hidden
  as before: 1280 × 675 pt.
- `board.template {name, x, y}`, cycling through the five ids, put each template's top-left at the
  top-left of its own 1600 × 1000 pt cell, six cells to a row, until `node.find {limit: 0}`
  reported 500 or more. No `edit.duplicate`. 26 templates made 501 nodes, 199 of them top-level,
  with 126 stickies and their shadows: the same mix as the 501-node pile, laid out. Zoom to fit:
  11 %.
- The `view.set` series centres on the fourth Brainstorm (row 3, column 5), because Brainstorm
  holds the most stickies of the five templates. At 100 % a render holds that template alone, with
  its 8 stickies. At 50 %, overscan included, it reaches into the eight templates around it: about
  three dozen stickies, at half the size.

Render times. Release build, nearest-rank percentiles. Each sample is the one `canvas render … ms`
line a command caused, and every command caused exactly one.

| Series | Zoom | Renders | p50 (ms) | p95 (ms) | min–max (ms) | `vm.loadavg` (1 min) |
|---|---|---|---|---|---|---|
| **Step 3**: `view.zoomFit`, then `view.zoomIn`/`view.zoomOut` ×10 | 100 % / 50 % | 20 | **244.0** | **536.8** | 228.2–543.2 | 7.25 → 6.28 |
| Fit: `view.zoomFit` after each `view.zoomIn` (Plan 1's method) | 59 % | 20 | 297.9 | 312.4 | 285.1–316.8 | → 3.81 |
| Edits: `node.set` one sticky's fill, two colours in turn | 59 % | 20 | **38.3** | **40.5** | 18.5–41.9 | 3.38 → 3.26 |
| Step 3's cycles on the laid-out board (fit 11 %) | 13 % / 6 % | 20 | 22.6 | 35.9 | 9.2–36.8 | 3.71 → 3.58 |

What the time goes on. This is diagnosis only, and nothing was changed. Each row renders the same
20 views, `view.set` to 100 % and 50 % about the fit centre, and the rows ran back to back. The
laid-out board's row ran later, in the third launch, with its views centred on the fourth
Brainstorm.

| Board | p50 (ms) | p95 (ms) | 100 %: p50 | 50 %: p50 | `vm.loadavg` (1 min) |
|---|---|---|---|---|---|
| The budget board | 241.7 | 543.3 | 529.6 | 237.7 | 8.83 → 7.06 |
| The same, every effect removed by one `node.set` (undone after) | 57.8 | 64.7 | 60.1 | 56.0 | 6.43 → 5.66 |
| An empty page (the page and its dots) | 14.9 | 15.4 | 14.9 | 14.6 | 7.06 → 6.43 |
| 501 nodes on a page of their own: the ten templates, one duplicate round, six more templates (199 top-level, fit 67 %) | 171.2 | 381.6 | 367.3 | 168.0 | 8.15 → 7.21 |
| The same 501 nodes laid out over their page: 26 templates, one to a 1600 × 1000 pt cell (199 top-level, fit 11 %), the views on the fourth Brainstorm | 43.0 | 47.0 | 43.0 | 42.8 | 3.61 → 3.29 |
| The real old board after its migration (Step 4: 182 nodes, no stickies) | 18.0 | 22.3 | 16.4 | 18.5 | 10.80 → 10.09 |

- The stickies' shadows take about 89 % of a settle render at 100 % and 76 % at 50 %. FigCraft
  draws each shadowed layer in a pixmap of its own and blurs it on the CPU, at device scale, for
  every layer in view, on every full render.
- Without them, the 776-node board is near the budget (p95 64.7 ms), and a board of shapes,
  connectors and text is well inside it.
- Laid out, the same 501 nodes settle inside the budget: p95 47.0 ms at 100 % and 50 %, where the
  pile took 381.6 ms, and 35.9 ms in Step 3's cycles, which run at 13 % and 6 % on that board. A
  repeat of each series gave 45.7 and 36.4 ms. So a settle's cost follows the number and size of
  the shadows in its render, not the board's node count.
- An edit stays inside the budget, because `Retained` redraws only what changed.

Idle: no input for 3.5 minutes, with the board on screen. `ps -o %cpu=`, 10 samples 1 s apart: 0.4,
0.4, 1.1, 0.3, 0.2, 1.5, 0.2, 0.3, 1.7, 0.3 (mean 0.6 %). `top`'s 1 s samples: 0.3–0.7 %. No
render line came while it sampled, so nothing redraws on its own. The app held 532 MiB (RSS) with
this board open.

Verdict against the spec's budget:
- A re-render after an edit: p95 40.5 ms. Passes.
- A re-render after a settle: p95 536.8 ms on the budget board, 381.6 ms at 500 nodes. Misses, by
  7 to 11 times. Both boards are piles, so these are upper bounds: nearly every sticky's shadow is
  in each render. Laid out over their page, the same 501 nodes settle inside the budget: p95
  47.0 ms at 100 % and 50 % on one template, and 35.9 ms in Step 3's cycles. The real board settles
  at p95 22.3 ms.
- An idle canvas: about 0 % CPU. Passes.
- Pan frames at display rate: synthetic scrolling can't reach the window, so this is hand check 2.
- Nothing was tuned (Step 3). The spec's Risks has the follow-up: "Sticky shadows make settles
  miss the budget when many stickies are in view".

Beside Plan 1 (P1.T7 delegate ruling): Plan 1 timed another document, the ridgeline example (a
design file with no sticky shadows), in a debug build under build load. Like with like, its fit
view took about 60–63 ms once the debug-only conversion is taken out. Those numbers aren't compared
with these.

Save cost (P7.T6 carry): `savecost` on a copy of the budget board's `canvas.figcraft` (777 nodes;
632 KB of document JSON, 32 KB saved). Best of 7, with the CPU 73–79 % idle.

| Versions | Read from the file | Made in this session | File |
|---|---|---|---|
| None | 2.1 ms | – | 32 KB |
| 20 | 23.0 ms (+20.9) | 41.4 ms (+39.3) | 682 KB |
| 100 | 103.4 ms (+101.2) | 196.4 ms (+194.3) | 3,280 KB |

At FigCraft's 100-version cap, an in-place autosave of a board this size holds the UI thread for
0.1–0.2 s. So the spec's Risks also has the writer-thread follow-up the P7.T6 ruling names: "In-place
saves encode on the UI thread".

R-EHFRAME: the debug app (`cargo build -p sawit`, 41 s) has 22,694,284 bytes (21.64 MiB) of
`__eh_frame`, against 3.56 MiB in release, and its link prints the known warning. The debug binary
is 287 MiB. Option (b), opt-level for all dependencies in the dev profile, is the user's choice. A
faster debug canvas also needs its own `[profile.dev.package.sawit-board]` entry, because option
(b)'s `"*"` covers dependencies, not workspace members (P1.T7 delegate).

Carried guards, all clean:
- `git grep -n -w -e SAWIT_NATIVE_CANVAS -e native_canvas -e with_native_canvas e52cf5f -- crates`
  prints nothing. `crates/mcp/tests/tools.rs` has no
  `without_the_native_flag_the_canvas_tools_are_as_they_were`. Both `native()` helpers build a
  plain `Sawit::new` (`crates/mcp/src/app.rs:1193`, `crates/mcp/tests/tools.rs:223`). These are
  the P8.T2 and P8.T1 delegates' guards.
- `git show main:crates/board/fonts/OFL-Inter.txt` is the OFL text that NOTICE and README name,
  4,470 bytes (P8.T3 delegate).

Step 4: a copy of the real old board, `~/Library/Application Support/sawit/canvas.excalidraw`
(167,774 bytes, 134 live elements, last changed 2026-10-30 13:37), as the second launch's
`.nyawit/canvas.excalidraw`.
- `open_page canvas` answered `{"tab":0}` in 297 ms, the import included. `get`: 86 top-level
  layers, as P7.T7 found: 31 roundedRectangle, 8 rectangle, 3 diamond, 2 text and 42 connectors (6
  with text, 38 joined at both ends, 4 at their end only). No notes; 182 nodes.
- The screenshot (25 %, sidebar and terminal hidden) shows the whole "sawit.dev — user flow" board:
  its boxes, diamonds, connectors and legend. It has the import's known losses (P7.T7): the two
  headings at 12 px, the legend's labels in ink, straight edges.
- The first autosave wrote `canvas.figcraft` (14,252 bytes). The copy's SHA-256 (`8adfe656…82b8`)
  was the same after the migration and after the app quit. The original, which was only read, kept
  it too.
- The copy and its screenshots were deleted after the check. They aren't committed.

Step 6: the instance was stopped first (`/bin/kill $PID`, P8-C12). `cargo fmt --all -- --check` and
`cargo clippy --workspace --all-targets -- -D warnings` are clean. `cargo test --workspace` passes:
1309 passed, 9 ignored, 0 failed (sawit 264, sawit-board 632, sawit-core 324 + 23, sawit-mcp 54 +
12). Their one other line is the `block 0.1.6` note. It is ready for the user's `/consolidate` once
the hand checks below pass and the user accepts the settle-budget miss (spec Risks). Nothing was
pushed.

Not failures, for the user:
- README says the binary is "about 34 MB", in its introduction and under "Size & memory". It is now
  57.6 MiB. Its "about 67 MB of memory when idle" wasn't measured again; with the budget board
  open, the app held 532 MiB.
- Two one-off pauses on the UI thread: the first `board.template` after launch took 300 ms (later
  ones 1–23 ms), and opening the canvas that migrated the real board took 297 ms.

##### Hand checks (pending — needs the user)

Build and run the release app: `cargo build --release -p sawit` in a checkout of main. Make the
scratch folders and a file for hand check 1 first:
`mkdir -p <scratch>/home <scratch>/proj <scratch>/proj2 && echo hi > <scratch>/proj/notes.txt`.
Sawit opens a path that isn't there as a file, in a window with no project, so without them both
windows of hand check 8 would show one canvas, `<scratch>/home/canvas.figcraft`. Then run
`SAWIT_HOME=<scratch>/home target/release/sawit --foreground <scratch>/proj`, and open the Canvas
page (⌃⌘C). The scratch home keeps your own Sawit data out of it. Where a step uses the control
socket, use `ctl` from Plan 1 Task 7's launch recipe. Checks 4 to 9 point at earlier plans' hand
checks, whose steps still hold on this build: their `SAWIT_NATIVE_CANVAS=1` no longer does
anything. Record each answer under its number.

1. **Opening, maximizing and the tab bar** (Step 5, P8-C8, and the P8.T1 delegate). First Plan 1
   hand check 4: ⌃⌘C and the title bar's Canvas button bring the canvas tab forward, with no second
   one. Then click empty canvas, so it has the keyboard, and edit no text:
   - ⇧⌘↩ hides the tab bar, and the canvas fills the window. A second ⇧⌘↩ puts the layout back.
   - Maximize again, then press ⌘B: the sidebar and the tab bar come back.
   - Open `notes.txt` from the Files tab, go back to the canvas tab and maximize again, then press
     ⌃Tab: the file's tab shows, with the tab bar.
   Answer: ______
2. **Panning the budget board** (P8-C13). Build it over the socket: run
   `ctl "{\"cmd\":\"canvas\",\"op\":{\"op\":\"command\",\"command\":\"board.template\",\"params\":{\"name\":\"$t\"}}}"`
   for each `t` in retrospective, brainstorm, weeklyPlanner, flowChart, kanban, retrospective,
   brainstorm, weeklyPlanner, flowChart, kanban. Then run
   `ctl '{"cmd":"canvas","op":{"op":"command","command":"select.all"}}'` and
   `ctl '{"cmd":"canvas","op":{"op":"command","command":"edit.duplicate"}}'`, twice. Then
   `ctl '{"cmd":"layout","sidebar":false,"terminal":false}'` and
   `ctl '{"cmd":"canvas","op":{"op":"command","command":"view.zoomFit"}}'`.
   - Pan with two fingers. The board follows at once, as the last frame moves with the fingers. A
     fast pan shows the page colour at the leading edge until the board fills in. Nothing jumps
     when a new frame lands.
   - Pinch to zoom. After the fingers stop, the board stays soft for about 0.4–0.7 s: the 150 ms
     wait for the zoom to settle, then the 0.2–0.5 s full render measured above. Say whether that
     is acceptable until the Risks follow-up lands.
   Answer: ______
3. **Bold Inter without Inter installed** (the P2.T7 carry; it replaces Plan 2 hand check 4's
   Helvetica Neue workaround). This Mac has no Inter installed. Put a sticky with the toolbar's
   sticky tool, type two words, select one and press ⌘B: that word turns bold on the canvas. Then
   select a text layer and show the Design panel (⌥8): Typography's family reads Inter, and its
   weights run from Thin to Black. Answer: ______
4. **Tools, flyouts, stickies, connectors and text with IME** (Step 5): Plan 3 hand checks 1–5, and
   Plan 2 hand checks 1, 7 and 10 (a Chinese input method). Answer: ______
5. **Layers: drag and rename** (Step 5): Plan 4 hand checks 1–4. Answer: ______
6. **Design panel fields** (Step 5): Plan 5 hand checks 1–8. Answer: ______
7. **Menus, the palette and the sheet** (Step 5): Plan 6 hand checks 1–3 and 10. Then, with a
   sticky selected, open the palette (⌘K), type "dupl" and press Enter: the sticky is duplicated.
   Answer: ______
8. **Copy and paste between two project canvases** (Step 5, Plan 2 hand check 12 (e)). Open the
   second folder in a window of its own:
   `SAWIT_HOME=<scratch>/home target/release/sawit -n <scratch>/proj2`, and open its Canvas page.
   Its canvas is `<scratch>/proj2/.nyawit/canvas.figcraft`, the first window's
   `<scratch>/proj/.nyawit/canvas.figcraft`.
   In the first window's canvas, select a sticky and press ⌘C. Click the second window's canvas
   and press ⌘V: the sticky pastes there, and ⇧⌘V pastes it again. Answer: ______
9. **Export** (Step 5): Plan 6 hand checks 6–8 and Plan 5 hand check 8. Answer: ______
10. **`canvas_add_mermaid` from Claude Code** (Step 5). Point Claude Code's Sawit MCP server at this
    build: a symlink named `sawit-mcp` to `target/release/sawit`, started with the same
    `SAWIT_HOME`. Or use the installed app once `/consolidate` has released it, with a scratch
    folder's window in front. With the canvas open, ask Claude Code to add
    `flowchart LR; A[Start]-->B{Ok?}; B-->|yes|C[Done]` to the canvas. Three editable shapes come in
    beside what is there, with two connectors joined to them, "yes" on the second, and the tool
    answers `added: 5`. With Sawit closed, the tool edits the saved file instead. That needs no
    hand check: sawit-mcp's offline tests cover it
    (`the_other_canvas_tools_edit_the_stored_canvas_while_the_app_is_closed`, P8.T2 delegate
    ruling). Answer: ______
11. **A real board, walked through** (the P7.T4 carry (3) and the cross-plan ruling). Copy the old
    board, never the original:
    `mkdir -p <scratch>/real/.nyawit && cp ~/Library/Application\ Support/sawit/canvas.excalidraw <scratch>/real/.nyawit/`.
    Run the release app on `<scratch>/real` and open the Canvas page: the user-flow board comes in
    whole. Say which of the import's losses matter, so the follow-up adds those Skeleton fields:
    heading sizes (36 and 24 px come in at 12 px), the legend labels' colours, Excalidraw's fonts,
    and curved routes (30 edges come in straight, and the four into "Workspace" meet at one point).
    Plan 7 hand check 2 compares the same board with the old canvas. Answer: ______
