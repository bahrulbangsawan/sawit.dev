# Sawit canvas vs FigCraft: what is not implemented yet, and what agents should build

**Date:** 2026-11-06 · **FigCraft pinned:** `8eb0b87646c5aef712326d6c431ccde5408630de` (v0.15.0; was `0971d09`, v0.12.1) ·
**FigCraft HEAD when written:** `8eb0b87646c5aef712326d6c431ccde5408630de` (2026-11-05), 445 commits ahead
(266 of them in the crates Sawit uses).

You are working on **Sawit**, a native macOS developer workbench in Rust on GPUI. Its canvas runs on
[FigCraft](https://github.com/storytold/figcraft)'s engine. Sawit uses FigCraft's `doc`, `engine`, `render`,
`text`, `format`, `color` and `geom` crates, pinned in the root `Cargo.toml`, and draws its own interface:

- `crates/board` (`sawit-board`) holds the document logic, with no GPUI, so it is tested headless.
- `crates/app/src/board` holds the GPUI views.

FigCraft's own interface (`crates/ui-egui`) is **not** used. Any FigCraft feature that needs a panel, an
overlay or a dialog has to be rebuilt here.

This file lists every FigCraft feature that the Sawit canvas does not offer yet, and says what to build for
each. The engine underneath already has almost all of them. Agents can reach any engine command through
`canvas_command` (`crates/board/src/ops/command.rs`), but people using the canvas cannot.

---

## Ground rules for agents

1. **Read before you change.** The design spec is `docs/superpowers/specs/2026-10-29-native-canvas-design.md`
   (decisions D1–D16). Its "Non-goals" list explains why most of what follows was left out. Lifting a non-goal
   is fine, but say so in the commit and update the spec.
2. **Logic in `crates/board`, views in `crates/app/src/board`.** Each new panel or overlay gets a model in
   `sawit-board`, with `tests/` beside it, and a thin view in the app. The existing sections (`design/paints`,
   `design/effects`, `design/layout`, `context_bar`, `overlay`) are the pattern to copy.
3. **Every edit goes through engine commands**, usually `node.set {ids, props}` or the command's own id, so it
   undoes in one step and agents can do the same thing. Never mutate the `Document` directly.
4. **The engine asks, the canvas answers.** A command that needs UI pushes a `UiRequest` into
   `session.ui_requests`. `crates/app/src/board/run.rs` (`handle_ui_requests`) handles only a few kinds and logs
   the rest at debug level, so those commands appear to do nothing. A feature is done only once its request
   kinds are handled there.
5. **Menu policy.** `crates/board/src/policy.rs` (`HIDDEN`, `HIDDEN_PREFIXES`) and
   `crates/board/src/menu/enabled.rs` (`NEEDS_INPUT`) hide or grey out commands. When you ship a feature,
   take its ids off those lists. Also take its ids off `NOT_OFFERED` in `crates/board/src/shortcuts/` so they
   get keys again.
6. **Keep the MCP server in step.** If agents would want a feature, extend the matching `canvas_*` tool in
   `crates/mcp/src/app/canvas/` and update the README's MCP table.
7. **Test in the real app.** Run the crate tests with `source /Volumes/bahrul/.toolchains/env.sh` first; all
   build output stays on the bahrul disk. Then launch Sawit and check with its own tools: `canvas_*`,
   `app_state`, and a window screenshot through `e2e/node_modules/agent-desktop/bin/agent-desktop screenshot
   --window-id <id>`. Unit tests alone are not enough for UI work.
8. **One item per commit**, using Conventional Commits (`feat(canvas): …`). Run `cargo fmt`, then
   `cargo clippy` and the tests of the crates you touched. Do not push or release unless asked.
9. **Destructive actions ask first. Popovers fit their content. Only the user moves the keyboard.** See
   `docs/notes/2026-11-06-backlog-prompt.md`; those rules apply here too.

---

## Status on branch `canvas/figcraft-gaps` (2026-11-06)

Done, with tests in `crates/board` and a clean `cargo clippy -p sawit --all-targets`:

- **0. FigCraft bump** to `8eb0b87` (v0.15.0): the dot grid's new fields, 23 board shapes, the
  connector previewed in the document, ⇧⌘\ now FigCraft's Minimize (hidden; the layers panel keeps
  ⌥1), PC's Menu key left out, new window-level commands hidden. The export-options workaround stays:
  `node.set exports` still snake-cases its keys upstream.
- **1. Route frames**: desktop by default, responsive, full page height measured at the viewport's
  height on every settle (grows and shrinks, no runaway on `100vh` pages), the fit is no undo step,
  resized frames lay out again without a reload, the old picture keeps its proportions until the new
  one is in, Full page toggles it.
- **2.1 Prototyping**: Prototype tab (interactions: trigger, action, destination, animation,
  duration; flows), connection lines, Present / Preview player (navigate, back, overlays, links).
- **2.2 Dev Mode** (⇧D): Inspect panel (properties with tokens, code in 10 languages with Copy,
  annotations, Ready for dev); annotated layers outlined on the canvas with their notes.
- **2.3 Variables**: binding loss fixed (per-row `fill.set` / `effect.set`), Variables window,
  bind menu on solid paints, per-collection mode menu on frames.
- **2.4 Components**: Component section (variants, booleans, text, instance swap; add / rename /
  delete properties, add variant; Swap, Reset, Detach, Go to main), Assets tab with pictures.
- **2.5 Paints**: kind menu (solid ↔ 4 gradients), angle, stops (position, colour, alpha, add,
  remove), image fill mode, tile scale, quarter turns, 7 adjustments, Choose… (`image.replace`),
  eyedropper (I).
- **2.6 Effects**: progressive blur, noise, texture and glass fields.
- **2.7 Layout**: constraints, min / max sizes, Ignore auto layout, grid columns and rows, layout
  grids section.
- **2.8 Typography**: decoration, case, list, wrap, paragraph spacing / indent, max lines,
  OpenType features, variable axes; Create link via its dialog.
- **2.9 Vectors**: Shape Builder tool with its regions, lasso, paint bucket area, vector region
  hatching.
- **2.10 Canvas aids**: layout grids, pixel grid, rulers with guides, slice / mask / frame outlines.
- **2.11 Boards**: timer and voting pill.
- **2.12 Comments**: Comment tool (C), pins, Comments window (reply, resolve, delete).
- **2.13 Input dialogs**: link, offset, move to page, batch rename, colour profile, nudge, find and
  replace, save version; Version history window; Set default properties and Set as thumbnail shown.

Still open:

- **2.14** live collaboration, branches, plugins and libraries (left hidden, as this file says).
- On-canvas handles: gradient handles, auto layout padding and gap handles, constraint guides,
  auto layout drop cells, the Scale tool's readout.
- Vectors: the Variable width tool and width profiles, the point panel (mirroring, per-point radius),
  `vector.offset` handles.
- Dev Mode: measurements kept on the canvas, Compare changes, dev resources, Code Connect.
- Prototype player: transitions other than instant (Smart animate), conditionals and variables in
  interactions, scroll behaviour, overlay positions.
- Comments: @mentions, reactions, read state.
- `edit.copyLink` and the missing-fonts download dialog stay hidden.
- Route frames: remote (non-localhost) pages after a confirmation.

---

## 0. Bump FigCraft first (do this before anything else)

- **What's wrong:** the pin is 445 commits behind. Upstream fixed bugs in exactly the crates Sawit ships:
  - a fill-distribution loop that could hang the app (#1926)
  - quadratic slowdowns on large documents (#1921)
  - mask nesting and GIF frame memory limits (#1925)
  - a saved file now keeps its permissions (#1957)
  - several stacked layer-blur bounds fixes (#1583, #1589, #1598, #1603, #1626)
- **New commands at HEAD:** `board.addConnected`, `board.grid`, `board.reshape`, `image.replace`,
  `layout.removeAll`, `library.check`, `pref.scrollWheelZoom`, `pref.uiScaleDown`, `pref.uiScaleReset`,
  `pref.uiScaleUp`, `text.openLink`, `text.wrapStyle`, `variable.extendCollection`, `view.minimize`,
  `widget.hover`.
- **Do:**
  - Move all seven `figcraft-*` revs in the root `Cargo.toml` together.
  - Copy FigCraft's NOTICE again into `NOTICE`.
  - Run `cargo test -p sawit-board -p sawit -p sawit-mcp`.
  - Re-check `crates/board/src/design/export/mod.rs`, which works around `node.set exports` dropping
    two-word options. Remove the workaround if upstream fixed it.
  - Decide per new command whether it belongs in `policy.rs` (`board.*`, `image.replace`,
    `layout.removeAll` and `text.wrapStyle` should be offered; `pref.uiScale*` and `view.minimize` hidden).
- **Done when:** all tests pass, and the shortcut sheet and menus list the new commands that are offered.

---

## 1. Live route frames (fixed on 2026-11-06; follow-ups)

**Already done:**
- Add Routes and `canvas_add_routes` make **one desktop frame (1440 wide) per route by default**. Other widths
  are opt-in.
- Each frame is **responsive**: its page is laid out at the frame's own size, so resizing the frame lays the
  page out again.
- Each frame **grows to the page's full height** once the page loads at 900 px tall (`WebData::fit`,
  `routes::fitted_height`, `ops::fit_to_page`, `crates/app/src/board/web/fit.rs`). The rows under it move
  down.
- Frames are named `Home · Desktop 1440` instead of `/ / 1440`.
- The empty band at the top of the section went from 480 to 200.
- The route bar has a **Full page** button that measures the page again.

**Follow-ups:**
- [ ] **Paint a snapshot at its own aspect ratio.** Between a resize and the next snapshot, the old picture is
  stretched to the new frame (`web/pool.rs`, `painted`). Paint it top-anchored at its own size, clipped to
  the frame.
- [ ] **Re-fit after a hot update.** Today a frame grows once from its base height. A page that gets longer
  later needs **Full page**. Measure the page height on each `sawit-web:dirty` settle, at the base viewport
  height in a hidden second view, so `100vh` layouts don't grow forever.
- [ ] **Remote pages.** Only localhost loads (`routes::localhost`). Allow `https://` hosts after a
  confirmation, and keep the refusal for `file:` and `javascript:`.
- [ ] **Undo of a fit.** "Fit to page" is its own undo step. Fold it into the "Add routes" step when it
  happens within the same render work (`routes/ops.rs`, `routes_rendered`).

---

## 2. Feature gaps, area by area

**Status key:**
- **Engine ✓, UI ✗:** the engine has it, Sawit shows nothing for it.
- **Partial:** some of it is shown.
- **Hidden:** the command is removed from the menus and keys by policy.

### 2.1 Prototyping (Engine ✓, UI ✗; a spec non-goal)
- Connections, triggers, actions, Smart Animate-style transitions, overlays, scroll behaviour, flows.
- Variables with conditionals (if / else if / else) and expressions (`count + 1`) in the interaction editor.
- Presentation view with device frames, hotspot hints, and a live preview pane.
- `proto.*` (21 commands); `proto.present` and `proto.preview` are in `NOT_OFFERED`.
- `view.prototype` and `view.designPrototype` are hidden.
- Request kinds with no handler: `panel.prototype`, `panel.designPrototype`, `present.options`, `preview.close`.

**Build:**
1. A **Prototype tab** in the Design panel (`crates/app/src/board/design_panel/`) listing the selection's
   interactions (trigger, action, destination, transition, duration, easing), with a model in
   `crates/board/src/design/prototype/`.
2. **Connection noodles** in the overlay (`crates/board/src/overlay/`): curves from hotspot to destination
   frame, which can be dragged to re-target.
3. **Present / Preview** reusing Screen Preview (`crates/app/src/board/screens/preview.rs`). FigCraft renders
   the prototype runtime (`figcraft_engine::proto` runtime), so draw its frames into the preview surface.
4. Flows list (`proto.flow.*`).

### 2.2 Dev Mode (Engine ✓, UI ✗; a spec non-goal)
- Inspect panel: properties, spacing, CSS / HTML / React / Vue / Leptos / SwiftUI / Compose / ArkUI / Flutter
  code, with component properties as parameters.
- Measurements kept on the canvas (`measure.*`), Alt-to-measure distances, annotations in colour-coded
  categories (`annotation.*`, 8 commands).
- Ready-for-dev status and list, focus view (`dev.*`), Compare changes (`view.devCompare`), Dev resources
  (`devResource.*`), Code Connect (`codeConnect.*`).
- `view.devMode` is in `NOT_OFFERED`. Request kinds `panel.devMode` and `view.devCompare` have no handler.

**Build:**
1. An **Inspect panel** that takes the Design panel's place while Dev Mode is on. Show the engine's
   generated code with a language picker and a copy button.
2. **Alt-hover redlines** in the overlay, using the engine's measure overlay data.
3. **Annotations and measurements** drawn on the canvas, plus a panel section to add and edit them.
4. Sawit's own `screens/` React/CSS export stays. Offer both, and say which is which.

### 2.3 Variables, modes and styles (Engine ✓, UI ✗; a spec non-goal)
- Colour, number, string and boolean variables with collections, modes, aliases and scopes; a Variables
  table (`variable.*`, 19 commands); bindings on fills, strokes, effects, grids and instance properties.
- Local styles: paint, text, effect and grid (`style.*`, 10 commands), plus style pickers.
- **Known bug:** editing a solid paint sends the list back as 8-bit hex and **drops a variable binding** that
  isn't on the 8-bit grid (`crates/board/src/design/paints/mod.rs`, header comment, review "m1"). Fix this
  first.

**Build:**
1. Fix the binding loss: send unchanged rows back with their `boundVariables`, or edit rows with
   `fill.set` by index.
2. A **Variables window or tab**: collections down the side, a grid of variables × modes, alias pickers.
3. A **bind** affordance on every paint, effect and number field (a hexagon icon), plus a mode switcher on
   frames (the `Theme` mode in FigCraft's Ridgeline example).
4. **Style pickers** next to Fill, Stroke, Effects, Typography and layout grids, using the `style.*` commands.

### 2.4 Components beyond the menu (Partial)
- **Already in Sawit:** create, create multiple, combine variants, detach, reset, go to main, push changes,
  restore main, all from the menus.
- **Missing:**
  - component properties (text, boolean, instance swap, variant) and their editor
  - the variant property panel on instances
  - swap instance
  - an Assets panel with live thumbnails, search and drag-to-canvas (`view.assets` hidden; request
    `panel.assets` unhandled)
  - slot badges
  - `component.hideWhenPublishing` (hidden)

**Build:**
1. A **Component section** in the Design panel: property list and editor for mains; variant dropdowns,
   boolean toggles, text fields and swap pickers for instances.
2. An **Assets panel** in the left sidebar, as a tab next to Layers, that renders thumbnails with
   `figcraft_render` off the UI thread.

### 2.5 Paints (Partial)
- **Already in Sawit:** solid fills and strokes with a picker, hex and opacity.
- **Missing:**
  - Editing linear, radial, angular and diamond gradients: stops, angle, on-canvas handles. Today a gradient
    row can only be removed or replaced with a solid (`crates/app/src/board/design_panel/paints/`).
  - Image fills: fill, fit, crop and tile modes, adjustments (exposure, contrast, …) and presets.
  - Per-paint blend mode in the panel.
  - Display P3 colour and the file colour profile (`file.colorProfile` is in `NEEDS_INPUT`).
  - Eyedropper (`edit.pickColor` hidden).

**Build:**
1. A gradient editor inside the colour picker (`paints/picker.rs`): a stop bar and a type switch.
2. Gradient handles in the overlay.
3. An image paint editor.
4. An eyedropper that samples the rendered page pixmap.

### 2.6 Effects (Partial)
- **Already in Sawit:** drop and inner shadow, layer and background blur.
- **Missing:**
  - editing noise (mono, duo, multi), texture and glass (refraction, dispersion, frost, light); today they
    are listed, hidden and removed only (`crates/board/src/design/effects/mod.rs`)
  - progressive blur controls in the panel

**Build:** one row editor per kind, from the engine's effect JSON keys.

### 2.7 Auto layout, constraints and grids (Partial)
- **Already in Sawit:** direction, wrap, gap, padding, 3×3 align, baseline, hug / fill / fixed.
- **Missing:**
  - Grid auto layout editing: fixed, hug and `fr` tracks, cell spans.
  - Min and max width and height.
  - Absolute position inside auto layout.
  - On-canvas padding and gap drag handles.
  - Auto-layout drop cells while dragging (not drawn).
  - **Constraints** UI (left/right/center/scale…) and constraint guides.
  - **Layout grids**: editing (`grid.*`) and drawing (`view.layoutGrids` hidden; the overlay draws none).

**Build:**
1. Track editors and min/max fields in `design/layout`.
2. A Constraints section.
3. A Layout grid section.
4. Grid, padding and gap handles in `crates/board/src/overlay/`.

### 2.8 Typography (Partial)
- **Already in Sawit:** family, style, size, line height, letter spacing, horizontal and vertical align,
  auto-resize.
- **Missing in the panel** (some are reachable from the Text menu):
  - paragraph spacing and indent, decoration, case, lists
  - OpenType features
  - variable font axes
  - max lines / truncation count
  - text styles
  - link editing (`text.link` is in `NEEDS_INPUT`)
  - missing-fonts dialog (`text.missingFonts` hidden)

**Build:** a "Type details" popover, as in FigCraft, holding those controls, and a small URL dialog for links.

### 2.9 Vectors (Partial)
- **Already in Sawit:** vector edit mode with points and handles, pen and pencil, booleans, flatten and
  outline from the menus, arc and corner-radius handles.
- **Missing:**
  - The **Shape Builder tool** (`tool.shapeBuilder` hidden; regions not drawn).
  - Overlays for the lasso, the paint bucket's area, and the box around several selected points.
  - The **variable-width stroke tool** and width-profile presets.
  - The point panel (handle mirroring, per-point corner radius).
  - `vector.offset` (in `NEEDS_INPUT`).

**Build:**
1. Draw the engine's tool overlay data for those tools.
2. Un-hide `tool.shapeBuilder`.
3. Add a point section to the Design panel while in vector edit mode.

### 2.10 Canvas aids (Hidden, because nothing draws them)
- Rulers and ruler guides (`guide.*`), pixel grid, slice outlines, mask outlines, frame outlines.
- The Scale tool's readout, constraint guides, swap targets.
- Names don't rotate with a rotated frame, and the caret doesn't blink (`overlay/mod.rs` header lists all of
  these).

**Build:** draw each in `crates/board/src/overlay/`, then remove its `view.*` toggle from `policy::HIDDEN`.

### 2.11 Boards (FigJam-like) (Partial)
- **Already in Sawit:** stickies, shapes, connectors, stamps, marker, highlighter, eraser, templates, tables,
  sections.
- **Missing:**
  - The shared **timer** and **voting**: request kinds `board.timer`, `board.timerState` and
    `board.voteResults` have no handler.
  - Cursor chat (`collab.cursorChat`, in `NOT_OFFERED`).
  - At HEAD: `board.grid`, `board.reshape`, `board.addConnected` (after item 0).

**Build:** a timer pill and a vote-results view as GPUI overlays driven by those requests.

### 2.12 Comments (Engine ✓, UI ✗; a spec non-goal)
- Threads pinned to frames, replies, @mentions, reactions, read state (`comment.*`, 15 commands).
- `tool.comment` and `view.comments` are hidden; request kinds `comment.*` have no handler.

**Build:**
1. Comment pins in the overlay, and a thread popover.
2. A comments list in the left sidebar.
3. Pair it with an MCP tool, so agents can read and answer comments (FigCraft's MCP has `comments`,
   `watch_comments` and `fix_comments` prompts).

### 2.13 Files, pages and editing commands that need input (Partial)
- **Greyed out (`NEEDS_INPUT`):** `text.link`, `vector.offset`, `object.moveToPage`, `object.batchRename`,
  `file.colorProfile`, `pref.nudge`.
- **Hidden:** `edit.findReplace`, `edit.copyLink`, `edit.setDefaultProperties`, `object.setThumbnail`,
  page thumbnails.
- **Version history and named versions** (`file.saveVersion`, `file.versionHistory`, `version.*`) are hidden.
  Sawit autosaves with `.bak` rotation instead.
- **Imports to check:** `.fig` (best effort), REST API JSON, Sketch and Penpot imports exist in
  `figcraft-format`. Confirm whether Sawit's `file.import` (a `UI_COMMANDS` entry, `crates/board/src/ops/files.rs`)
  accepts them, and if not, wire them in.

**Build:** one small dialog component, reused for each `NEEDS_INPUT` command, with a page picker for Move to
page. Then a Find & Replace bar, and Version history as a list over the `.bak` files plus engine versions.

### 2.14 Collaboration, branches, plugins and libraries (Hidden; the spec's non-goals)
- Live sessions, multiplayer cursors, spotlight, share links (`collab.*`, 21 commands); branches
  (`branch.*`, 7).
- Plugins (`plugin.*`, `plugins.*`; WebAssembly sandbox) and team libraries (`library.*`, `libraries.*`).
  The whole families are hidden by `HIDDEN_PREFIXES`.

**Do:** leave these until a spec says otherwise. Libraries come first, because they unlock design-system reuse
across project canvases.

---

## 3. Suggested order

1. Item 0 (FigCraft bump).
2. 2.3, fix only the variable-binding loss bug.
3. 2.13 `NEEDS_INPUT` dialogs. Cheap, and they turn six greyed-out commands on.
4. 2.5 gradient and image editing, then 2.6 effect editors.
5. 2.7 constraints, layout grids and on-canvas handles, plus 2.10 rulers and guides.
6. 2.4 component properties and the Assets panel.
7. 2.3 variables UI and style pickers.
8. 2.2 Dev Mode Inspect and code.
9. 2.1 Prototyping and Present.
10. 2.12 Comments.
11. 2.9 and 2.11, the remaining tools.
12. Section 1's route-frame follow-ups, in between, as they come up.

For each item, write a short plan first: the files, the model, the commands it uses and the requests it
handles. Then build it test-first in `crates/board`, then the view, then check it in the running app.
