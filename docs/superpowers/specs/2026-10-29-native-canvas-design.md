# Native canvas on the FigCraft engine

**Date:** 2026-10-29 · **Status:** approved for planning (decisions delegated to Claude by the user)

## Goal

Replace the Excalidraw canvas (React in a WKWebView, `crates/app/src/canvas.rs` + `web/canvas/`) with a
native GPUI canvas driven by the FigCraft engine (pure Rust, MIT OR Apache-2.0), with a toolbar, layers
panel, Design panel, menus and shortcuts. Every entry point keeps working (⌃⌘C, the title-bar button,
`app_open_page canvas`, the eight `canvas_*` MCP tools). When the native canvas covers what the old one
did, Excalidraw, the webview (`wry`) and the web bundle are deleted.

## Non-goals (this spec)

- Live collaboration, sharing, server publish, branches, version history.
- Comments, prototyping, Dev Mode / inspect, plugins, libraries and the assets panel, variables and modes,
  style pickers, a component-properties panel (component commands stay reachable from the menus),
  layout-grid editing, gradient-stop editing, eyedropper, find and replace, the missing-fonts download
  dialog, page thumbnails, several documents in one tab.
- The hand-drawn ("sketchy") look. It needs a hook inside FigCraft's renderer; it gets its own spec after
  cutover. It is the one Excalidraw trait this replacement does not carry over.
- Platforms other than macOS.

## Verified facts this design rests on

- FigCraft at rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3` (v0.12.1) compiles inside Sawit's tree on
  Rust 1.99.1 (`cargo check -p sawit` passed with `figcraft-engine`, `-render`, `-doc`, `-geom`). It adds
  68 crates and no version conflicts. A few font and SVG crates (skrifa, tiny-skia, usvg) end up in two
  versions.
- The engine is UI-agnostic. Everything is `Session::execute(id, &Value) -> Result<Value, EngineError>`.
  Pointer tools go through `Session::pointer(&PointerEvent)`. Text editing is `figcraft_engine::textedit::*`,
  including IME. The keymap (`keymap::KEYS`, `keymap::lookup`) and the menu bar (`catalog::MENU`, 246 items)
  are data. The context menu and the panels are UI code in `crates/ui-egui`, so we write our own.
- The renderer is CPU-based (tiny-skia). `figcraft_render::Retained::render(&Arc<Document>, page, &View,
  &Options) -> Option<(View, &Pixmap, Work)>` re-renders only what changed. It produces premultiplied RGBA.
  The board dot grid comes from `Options.dots`.
- Change detection works by polling: `DocState::revision`, `Arc::ptr_eq(&ds.doc, ..)`,
  `figcraft_text::layout_generation()`. Commands ask the UI for things through `Session::ui_requests`.
- File access is unconfined unless the caller uses `file_access::confined`.
- In gpui-pre 0.3.10, **key bindings dispatch before `on_key_down`**. A view takes a key away from an outer
  binding by binding it to `NoAction` in its own key context. `TerminalView::bindings()` already does this.
- Paint primitives: `Window::paint_image(bounds, image_bounds, corners, Arc<RenderImage>, frame, grayscale)`
  and `Window::drop_image`. `RenderImage` frames are BGRA (see `viewers/pdf.rs`).
- Text input goes through `EntityInputHandler` + `ElementInputHandler` + `window.handle_input`.
- Sawit's Mermaid renderer (`sawit_core::mermaid::render`) runs Pele in QuickJS and returns a flattened SVG
  whose palette is already applied. `web/canvas/src/pele.ts` holds the SVG → editable-shapes converter that
  we port.

## Decisions

| # | Decision | Why |
|---|---|---|
| D1 | FigCraft crates are workspace git dependencies pinned to rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`: `figcraft-engine`, `-render`, `-doc`, `-geom`, `-text`, `-format`, `-color`. A version bump is a deliberate task. | Alpha project that changes daily. |
| D2 | A new GPUI-free crate, `crates/board` (package `sawit-board`), holds all document logic: store, view maths, input routing, overlay display list, panel and menu models, control ops, Mermaid and Excalidraw conversion. `crates/app/src/board/` holds only GPUI views. `crates/mcp` depends on `sawit-board` for the offline `canvas_get`. | Logic gets headless TDD; the views stay thin. |
| D3 | Render on a dedicated thread with `Retained`. Convert to BGRA and wrap in `RenderImage`, paint with `paint_image`, and release replaced frames with `drop_image`. During pan and zoom, re-project the last frame to its new screen rectangle until the new one arrives. Overscan is 256 logical px on each side. While a zoom gesture is active, render at 0.5× and render at full quality 150 ms after the last zoom event. Render with an opaque page background, so premultiplied and straight alpha are the same. | Frame rate doesn't depend on raster speed. |
| D4 | Overlays (selection, handles, hover, marquee, guides, size label, frame labels, caret, text selection, connector ends, pen preview) are a display list built in `sawit-board` from engine state and painted by GPUI in logical px. The colours are fixed canvas colours (selection `#0D99FF`, component `#9747FF`, guide `#F24822`), not theme colours. | Testable without GPUI; readable over any content. |
| D5 | The project canvas lives at `<project>/.nyawit/canvas.figcraft`, or `<app root>/canvas.figcraft` without a project, and is a `FileKind::Board`. Autosave runs 300 ms after the document changes, on a writer thread: newest save wins, tmp + rename. A `.bak` is written before a save that empties a non-empty canvas. The first open with no `.figcraft` but an existing `canvas.excalidraw` imports it once and leaves the `.excalidraw` file untouched. | Same behaviour as today, plus migration. |
| D6 | During build-out the native canvas is opt-in with env `SAWIT_NATIVE_CANVAS=1`, read once into `Workspace::native_canvas`. Plan 8 removes the flag and the old canvas. Nothing is pushed until cutover, because a push to `main` runs the release hook. | `main` stays releasable. |
| D7 | The FigCraft keymap is the source of truth. The "Board" key context (and "BoardText" while editing text) binds contested keys to `NoAction` so they reach the canvas. The conflict policy is the table below. Commands hidden by the menu policy can't be reached by keyboard either. | One shortcut source, explicit conflicts. |
| D8 | The menus are an in-canvas main menu (≡ button) generated from `catalog::MENU` under the menu policy below, a Sawit-defined context menu, an Actions palette on ⌘K and ⌘/, and a keyboard-shortcuts sheet (⌃⇧?). | FigCraft's own menu data; the context menu is UI code upstream. |
| D9 | The agent API keeps the eight `canvas_*` tool names and the element-skeleton JSON, and adds `canvas_command {command, params}` and `canvas_commands`. Ids are FigCraft node ids as strings. An agent's own skeleton id is stored in `pluginData["sawit.id"]` and works wherever an id is accepted; an agent id wins over a numeric node id. | Existing agents keep working; the full engine becomes reachable. |
| D10 | Mermaid goes `sawit_core::mermaid::render` → `sawit_board::mermaid::to_skeletons` (a Rust port of the `pele.ts` converter, using `roxmltree` + `kurbo`) → board shapes with connectors bound to them. Diagram types the converter does not edit (pie, gantt, …) come in as an editable SVG frame through `file.import`. | The same result as today, without a browser. |
| D11 | Tab layout: a top bar (36 px); the layers panel on the left (240 px); the Design panel on the right (264 px); the canvas in the middle; a floating toolbar at the bottom centre; a floating context bar above the selection. Boards open with both panels hidden, design files with both shown. ⌘\ (`view.toggleUi`) hides or shows all the chrome. | FigJam/Figma familiarity. |
| D12 | The chrome uses the Sawit theme (`cx.theme()`). The canvas calls `session.set_ui_dark(theme.mode.is_dark())` every frame. Canvas colours are the document's. | FigCraft's own rule. |
| D13 | `figcraft_text::scan_system_fonts()` runs once per process on a background thread from `board::init`. Every board re-renders when `layout_generation()` changes. | The font DB is global. |
| D14 | `.figcraft` files from the file tree open in a canvas tab bound to that path and autosave in place. `.excalidraw` files open imported and save to a sibling `<stem>.figcraft`. | The engine supports this for free. |
| D15 | Engine errors become a toast in the canvas (bottom, 3 s), never a panic. Direct engine calls other than `execute` (`pointer`, `textedit::*`) go through `figcraft_engine::guard::run`. | Matches FigCraft's never-crash rule. |
| D16 | FigCraft's NOTICE goes into a new top-level `NOTICE` and the README credits. ArtCraft logos are not used. Icons are Sawit's Lucide set. | Licence and trademark compliance. |

## Keyboard conflict policy

These are all the collisions between FigCraft's Mac bindings and Sawit's workspace bindings, computed from
`keymap::KEYS` and `workspace::bindings()` on 2026-10-29.

| Keystroke | FigCraft | Sawit | Who gets it when the canvas has focus |
|---|---|---|---|
| ⌘W | file.close | Close tab | Sawit |
| ⌘O | file.open (hidden) | Open folder | Sawit |
| ⌃Tab, ⌃⇧Tab | doc.next/previousTab | Next/previous tab | Sawit |
| ⌃⌘M | object.mask | Open MCP page | Sawit (Mask stays in the Object and context menus) |
| ⌥⌘O | vector.outlineStroke | Add project | Sawit (Object menu) |
| ⌘J, ⇧⌘J | vector.join, smoothJoin | Toggle terminal/center | Sawit (Vector menu) |
| ⌘B | text.bold (text) | Toggle sidebar | Canvas while editing text, else Sawit |
| ⌥⌘T | text.alignCenter (text) | Close other tabs | Canvas while editing text, else Sawit |
| ⇧⌘↩ | board.nextSticky (text) | Maximize pane | Canvas while editing text, else Sawit (it maximizes the canvas) |
| ⌘K | app.actions; text.link (text) | ⌘K chords | Canvas. The ⌘K chords are unavailable while the canvas has focus, as with Excalidraw today. |
| ⌘0 | view.zoom100 | Show prompts | Canvas, as today |

Any gpui-kit `Root` binding that a FigCraft binding uses (Tab, ⇧Tab, ⌘C, …) is bound to `NoAction` in
"Board". A GPUI test enumerates `keymap::KEYS` and proves every non-Sawit key reaches the canvas.

## Menu policy

- **Top menus shown:** File, Edit, View, Object, Text, Arrange, Vector, Preferences.
- **Help:** shows only "Keyboard shortcuts".
- **Never shown:** Plugins, Libraries and Tools. Tools live in the toolbar.
- **Hidden ids** (out of scope, or Sawit owns them). The same list is blocked from the keyboard:
  - **File:** `file.new`, `file.newBoard`, `file.newFromTemplate`, `window.new`, `file.open`,
    `file.openRecent`, `file.close`, `file.saveVersion`, `file.versionHistory`, `file.duplicate`,
    `file.rename`
  - **Collaboration:** `branch.create`, `collab.join`, `collab.publish`
  - **Edit:** `edit.copyLink`, `edit.findReplace`, `edit.pickColor`, `edit.setDefaultProperties`
  - **View:** `view.comments`, `view.multiplayerCursors`, `view.assets`, `view.prototype`,
    `view.themeLight`, `view.themeDark`, `view.themeSystem`, `view.fullscreen`, `view.panelWidths`
  - **Object and Text:** `object.setThumbnail`, `text.missingFonts`
  - **Preferences, plugins and libraries:** `app.preferences`, `plugins.*`, `plugin.*`, `libraries.*`
  - **Components:** `component.hideWhenPublishing`
  - **Help and tools:** `help.gettingStarted`, `help.page`, `help.about`, `tool.comment`,
    `tool.shapeBuilder`
  - **Document tabs:** `doc.nextTab`, `doc.previousTab`
- **UI-handled ids** (Sawit implements them instead of `execute`):
  - **Import:** `file.import`, `file.placeImage` (open dialog)
  - **Export:** `file.export`, `file.exportSvg`, `file.exportFramesPdf` (write to `.nyawit/exports/`,
    then reveal in Finder)
  - **Save:** `file.save` (flush the autosave now), `file.saveAs` (save dialog, writes a `.figcraft` copy)
  - **Copy as:** `edit.copyAsText`, `edit.copyAsCss`, `edit.copyAsSvg`, `edit.copyAsPng` (system clipboard)
  - **Panels:** `view.layers`, `view.design`, `view.toggleUi`
  - **Dialogs:** `help.shortcuts`, `app.actions`
- **Everything else** runs through `execute`. "Enabled" means the engine knows the command id or it is a
  UI-handled id. "Checked" comes from `figcraft_engine::cmd::view::checked`.

## Context menu (Sawit-defined)

- **Over a selection:**
  - Cut, Copy, Paste here, Duplicate, Delete
  - Bring to front, Bring forward, Send backward, Send to back
  - Group, Ungroup, Frame selection, Add auto layout
  - Flip horizontal, Flip vertical
  - Show/Hide, Lock/Unlock
  - Copy as ▸ Text, CSS, SVG, PNG
  - Select all
- **On empty canvas:** Paste here, Select all, Zoom to fit, Show/Hide UI.

A right-click on an unselected layer selects it first. The selection target comes from
`figcraft_doc::geometry::select_target` and `tools::label_at`, as `ui-egui/src/canvas/input.rs:269` does.

## Toolbar and context bar

- **Board toolbar groups:**
  - Move (V) / Hand (H)
  - Marker (M) / Highlighter (U) / Eraser (E)
  - Sticky (Q): flyout of 8 colours
  - Shapes (B): flyout of the 7 `BoardShape`s
  - Connector (X): flyout straight / elbow / curved
  - Text (T)
  - Section (⇧S)
  - Stamp (W): flyout of 6 stamps
  - Templates & table picker (`board.templates`, `board.template`, `board.table`)
- **Design toolbar groups:**
  - Move (V) / Scale (K) / Hand (H)
  - Frame (F) / Section (⇧S) / Slice (S)
  - Rectangle (R) / Line (L) / Arrow (⇧L) / Ellipse (O) / Polygon / Star
  - Pen (P) / Pencil (⇧P)
  - Text (T)
- **Context bar on boards** (port of `ui-egui/src/canvas/board_bar.rs`):
  - Stickies and shapes: colours (`BOARD_COLORS`), shape (`BoardShape::ALL`), font size
    (`board::TEXT_SIZES`)
  - Connectors: style plus start and end caps
  - Ink: colour
  - Mixed selections show only the controls every selected item shares.
  - The bar is clamped inside the canvas.
- **Top bar:**
  - ≡ main menu
  - Document name ("Canvas" for the project canvas, else the file name)
  - Zoom menu: zoom in, out, to fit, to selection, 50 %, 100 %, 200 %
  - Toggles for the layers and Design panels
  - Maximize, which reuses the `ToggleCanvasMaximize` event

## Layers panel

- **Pages:** list, add, rename, duplicate, delete, select.
- **Layer tree:**
  - Each row has a kind icon, name, eye and lock (eye and lock appear on hover or when off).
  - Click selects. ⇧-click selects a range, ⌘-click toggles a layer.
  - Double-click renames. An empty name keeps the old name.
  - Rows expand and collapse.
  - Dragging reorders and reparents through `node.reparent`. A drop into the layer's own subtree is
    rejected.
  - A name filter keeps the ancestors of matching layers.
  - Hovering a row highlights the layer on the canvas (`ds.hover`).
- The list is virtualized (`uniform_list`).

## Design panel

- **Sections, in order:**
  1. Header: kind and name, with a count when several layers are selected
  2. Position and size: X, Y, W, H, rotation, constrain proportions, align and distribute buttons
  3. Auto layout: add/remove, direction (horizontal, vertical, wrap), gap, padding (uniform or per side),
     the 3×3 alignment grid, hug/fill/fixed sizing for W and H
  4. Appearance: opacity, corner radius (uniform or per corner), blend mode, visibility
  5. Fill: list of paints; solid ones edited with a colour picker, hex and opacity; gradient and image
     paints shown as swatches that can be removed or replaced by solid
  6. Stroke: paints, weight, align (inside, centre, outside), dashed on/off
  7. Effects: drop and inner shadow (x, y, blur, spread, colour), layer and background blur (radius);
     add, remove, show/hide
  8. Typography: family (searchable list from `figcraft_text::family_names()`), weight/style, size,
     line height, letter spacing, horizontal and vertical align, auto-resize
  9. Export: the layer's export settings (format, scale) and an Export button
- **Mixed values** show as "Mixed", and an edit applies to every selected layer.
- **Writes** go through `node.set {ids, props}`; prop keys come from `crates/engine/src/cmd/props.rs`.
- **Scrubbing** a number label is one undo step: `begin_interaction` → `preview` → `commit_interaction`.
- **Invalid input** is rejected, and the field shows the current value again.
- **Board items** hide the panel by default (D11). When it is shown, it works the same as for design
  layers.

## Agent wire contract

`{"cmd":"canvas","op":{...}}` on the control socket. The native canvas answers synchronously.

| Op | Params | Result |
|---|---|---|
| `get` | – | `{ok, elements:[Summary]}` |
| `add` | `{elements:[Skeleton]}` | `{ok, added, ids}` |
| `mermaid` | `{source, x?, y?}` | `{ok, added, ids}`; placed right of the existing content with a 100 px gap when x/y are omitted |
| `update` | `{elements:[{id, ...}]}` | `{ok, updated, missing}` |
| `delete` | `{ids}` | `{ok, deleted, missing}`; connector labels go with their connector |
| `clear` | – | `{ok}` |
| `replace` | `{scene}` (Excalidraw JSON) or `{path}` (`.figcraft` / `.excalidraw`) | `{ok, elements}` |
| `export_svg`, `export_png` | – | `{ok, svg}` / `{ok, png_base64}` of the whole page |
| `command` | `{command, params?}` | `{ok, result}` |
| `commands` | – | `{ok, commands:[{id, label, params}]}` |
| `pointer` | `{kind: down\|move\|up\|double, x, y (page), shift?, alt?, cmd?}` | `{ok}` |
| `key` | `{key, shift?, alt?, cmd?, ctrl?}` | `{ok, handled}` |
| `type` | `{text}` | `{ok}`; types into the text being edited |

Failures look like `{ok:false, error}`.

**Summary** is `{id, type, x, y, width, height, text?, color?, from?, to?, name?}`. `type` is one of:
`rectangle`, `roundedRectangle`, `ellipse`, `diamond`, `triangle`, `speechBubble`, `chevron`, `sticky`,
`connector`, `ink`, `stamp`, `text`, `section`, `frame`, `image`, `vector`, `group`, `table`, `other`.

**Skeleton** follows today's MCP schema: `type`, `id?`, `x`, `y`, `width?`, `height?`, `label.text`,
`text`, `start.id`, `end.id`, `strokeColor`, `backgroundColor`, `roundness`. It also gains `sticky`.
The types map as follows:

| Skeleton type | FigCraft |
|---|---|
| rectangle (with `roundness`: roundedRectangle), ellipse, diamond | `board.shape {shape, x, y, width, height, text, color}` |
| sticky | `board.sticky {x, y, text, color}` |
| text | `node.create {type: text, x, y, text}` |
| arrow | `board.connector {from, to, endCap: triangleArrow, label}`; an unbound end becomes `{x, y}` |
| line | `board.connector` with both caps `none`; more than 2 points becomes `board.ink {points}` |
| frame | `node.create {type: section, ...}`; children named by `frameId` are reparented into it |
| image (Excalidraw import) | `file.import {data, name}` |
| freedraw (Excalidraw import) | `board.ink {points}` |

Colours are applied as follows:
- `backgroundColor` → `board.setColor`, or fill for non-board nodes. Transparent means no fill.
- `strokeColor` → the stroke paint through `node.set`.

Unknown element types are skipped. They are reported in `notes`, never as an error.

## Performance budget

Measured on Apple silicon, release build, a 2560×1600-device-pixel canvas showing a 500-node board:

- Pan frames are presented at display rate through re-projection.
- A re-render after an edit or settle takes ≤ 50 ms at p95.
- An idle canvas uses about 0 % CPU (no continuous redraw).

Plan 1 adds a debug log of render time per frame. Plan 8 measures the budget and records it in the plan.

## Testing

- **`sawit-board`:** headless unit tests on a real `Session`. This covers most of the logic.
- **App:** `#[gpui_kit::test]` tests check key-binding resolution in the Board contexts, entity
  construction and the input-handler glue. No pixel tests.
- **Live checks** follow the memory recipe:
  - Launch the debug app with a scratch `SAWIT_HOME` and `SAWIT_NATIVE_CANVAS=1`.
  - Drive it through the control socket (`canvas` ops).
  - Screenshot the window with `screencapture -l`.
  - Synthetic clicks don't reach the window, so input is driven through the `pointer`, `key` and `type`
    ops. Those run the same code path as GPUI's events after event decoding.
- **Gates for every commit:** `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and the touched crates' tests. Each plan ends
  with `cargo test --workspace`.

## Milestones

| Plan | Delivers | File |
|---|---|---|
| 1 Foundation | `sawit-board` crate, store and autosave, render worker, view-only `BoardView` with pan/zoom and dot grid behind the flag, control ops `get` and `command` | `plans/2026-10-29-native-canvas-1-foundation.md` |
| 2 Editing | Pointer tools, overlays, cursors, keyboard routing and the conflict policy, on-canvas text and IME, clipboard, ops `pointer`/`key`/`type` | `…-2-editing.md` |
| 3 Toolbar | Top bar, board and design toolbars with flyouts, context bar, templates and table picker | `…-3-toolbar.md` |
| 4 Layers | Pages and layer tree with select, rename, visibility, lock, drag, filter, hover | `…-4-layers.md` |
| 5 Design panel | Sections 1–9 with mixed values and scrubbing | `…-5-design-panel.md` |
| 6 Menus | Main menu, context menu, Actions palette, shortcuts sheet, UI-handled commands | `…-6-menus.md` |
| 7 Agents & import | Skeletons, all control ops, MCP tools (+ `canvas_command`, `canvas_commands`), Mermaid port, Excalidraw import and migration, `.figcraft`/`.excalidraw` file tabs | `…-7-agents.md` |
| 8 Cutover | Flag removed; Excalidraw, wry and the web bundle deleted; docs, NOTICE, budget measured | `…-8-cutover.md` |

## Risks

- **FigCraft API drift.** The rev is pinned, and every upgrade is its own task with the full test suite.
- **CPU raster at Retina sizes.** Mitigated by re-projection, `Retained`, half-resolution while zooming
  and overscan. If the budget fails, page-anchored tiles become the follow-up.
- **Atlas growth.** Every replaced frame is released with `drop_image`. The worker never holds more than
  two frames.
- **Uncommitted work in `main`.** About 1,500 lines across 29 files, including `workspace.rs`,
  `control.rs`, `canvas.rs` and `mcp/app.rs`. It has to be committed before Plan 1 starts, because the
  plans edit the same files.
- **A second untracked worktree (`browser-pane`) uses `wry`.** Plan 8 removes `wry` from `crates/app`;
  that branch has to add it back when it merges.
