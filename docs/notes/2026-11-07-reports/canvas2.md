# Track `canvas2` report (B15 second pass, B12 leftovers)

Worktree `/Volumes/bahrul/project/sawit-wt/canvas2`, branch `wt/canvas2` (from `backlog` at
816b6923). Target dir `target-canvas2` (its deps seeded from `target-canvas` to skip a cold
build). Commits are listed at the end, in order.

Skills loaded for the design work: `better-interface` and all six domain skills
(`better-accessibility`, `better-layout`, `better-writing`, `better-typography`, `better-colors`,
`better-ui`), plus `sawit-canvas-design`. Design sketch on the canvas first:
`docs/design/b15-panels.sawit` (Design panel for a screen, compact Layers panel, Preview bar with
zoom / device frame / Open in Browser, the Custom size fields), exported to PNG and checked.

## 1. B15: Convert to Layers, and layers as UI's default — done

- **Capture (JS, in the screen's own web view; must stay JS):**
  `crates/board/src/screens/layers/capture.js` reads the rendered page: each element's box,
  computed styles (fill, border, radii, opacity, shadow, flex direction / gap / justify / align,
  padding, grow), its text (runs of phrasing content, text nodes in mixed content, field values
  and placeholders), pictures (img/canvas/video as PNG data URLs, inline SVG with paints
  resolved), names (aria-label, alt, id, first class), and counts what it can't carry (pseudo
  content, background images, outside pictures).
- **Plan (Rust, `layers/plan.rs`):** wrappers that draw nothing around one child are lifted;
  boxes that draw or group become frames; auto layout where a flex row/column or a block's flow
  places children at an even gap (space-between too), padding from CSS (or the children's common
  margin), cross alignment from `align-items` or the geometry; children fill across when as wide
  as the inner box, grow along when `flex-grow`, one-line texts hug, text heights hug; out-of-flow
  children stay absolute.
- **Build (Rust, `layers/build.rs`, `layers/looks.rs`):** one undo step (`doc::grouped`): frames,
  text (font stack resolved to a family the canvas has, the system stack to SF Pro), images
  (`file.placeImage`), SVG as editable vectors (`file.import`), fills/strokes/radii/shadows;
  the page's colours become variables named by role (`text/1`, `bg/2`, `border/1`, or an
  existing token of the same value) and type sizes `type/16`, all bound; unnamed frames are named
  by role (Header, Navigation, Button, List item, Row, Column, Box, Group — never "Frame"). The new
  frame goes 100 right of the screen with its name; the screen frame becomes "<name> / HTML" and
  stays with its folder.
- **App:** `screens/convert.rs`: waits for the screen's view to be built and at rest (asks the
  pool for a view if it let go of it), evaluates the capture, runs the `screen_layers` op, selects
  the new frame, toasts "Converted to N layers. The HTML screen is kept beside it." (and what it
  couldn't convert). Entry points: the screen bar's **Convert to Layers**, the Design panel's
  primary **Convert to Layers**; the Layers row's HTML tag says how.
- **MCP:** new tool `canvas_screen_to_layers {work | screen}` (needs the app; with it closed it
  says how to start it). B4: `APP_CONTROLS` maps Convert to Layers to it. Skill text in
  `sawit-canvas`; README MCP table + tool count 176 → 177 (README and site).
- **Layers default:** INSTRUCTIONS, code-mode instructions, `canvas_begin`'s description and
  `kind`, the `sawit-canvas` skill and the design guide's cnv.1 / cnv.3 now say UI is a frame of
  editable layers built from the file's components (starter system first); `kind: "screen"` is for
  a live web prototype the user asks for. A screen-sized layer frame's `canvas_begin` answer now
  says `build: …` (instances, auto layout, tokens) instead of steering to HTML.
- **Tests:** unit `screens::layers::tests` (plan keeps rows/columns/pinned badge; uneven gaps keep
  places; build makes an editable frame with variables bound and one undo removes it; fonts,
  colours, shadows), e2e `screens-layers.test.js` (agent writes Today, converts: 39 layers, 25
  texts, 13 auto layouts; the HTML frame renamed; one undo takes it all back). Screenshot checked:
  the converted frame matches the HTML screen and shows the whole page height.

## 2. B15: panels redesign — done

- **Design panel fits the selection:** a screen shows only Position (X, Y, W, H — no rotation or
  align, which don't apply to a page) and a **Screen** section: what it is ("Its content is a web
  page. Convert it to layers to edit each text and button here."), its size as a device (D1's 20
  presets grouped Mobile / Tablet / Laptop / Desktop, checked), **Convert to Layers** (the one
  primary), **Preview** and **Export ▾** (HTML, React, PNG). No Fill, Auto layout, Resizing, Layout
  grid, Typography, Export settings (the open MEDIUM from the canvas track — fixed).
- Header: kind ("Screen"), the name cut short with its full text in a tooltip, an HTML tag.
- Text: Typography and Fill first, then Position. Components keep their properties first.
- Sections fold: each title is a small ghost button with a chevron (keyboard reachable, named
  "Title, open/folded"); a folded section is one row.
- Prototype tab: a **Device** section at its top (the device Present shows frames in, Landscape for
  devices that turn), then Interactions and Flows as before.
- **Layers panel:** Find moved to the panel's head (under Layers / Assets); pages are compact 28 px
  rows inset with rounded corners, a page icon, the page on show marked with a check; layer rows
  inset and rounded; the HTML tag's tooltip says how to edit.
- Reuses B16's parts: `menu_icons` (new `DEVICE`), `FloatBar`/`BarButton` (the screen bar's new
  button), the shared popup menu.

## 3. B15: Preview — done

`screens/preview.rs` (state, placement), `preview_bar.rs` (bar and stage), `preview_ops.rs`
(`app_state` and e2e drivers).
- **Size menu:** As Made, Window, the 20 devices grouped by kind, **Custom size…** (width and
  height typed in the bar, 200–3840, Enter/Apply, × cancels; a wrong value keeps the fields and says
  "Enter a width and height from 200 to 3840" with an alert icon in a strip the page leaves free).
- **Zoom:** Fit (never up) / 100% (one CSS px a point; a taller page scrolls inside the room; 100%
  is disabled with a tooltip when the window is narrower than the page).
- **Device frame:** a bezel with a shadow round phone and tablet sizes; off (with why) for laptop
  and desktop sizes.
- **Open in Browser:** opens the screen's `index.html` in the project's Browser tab.
- Rotate, Reload, Close; icons everywhere; no scrolling row; at a tight canvas (< 600 pt) the zoom is
  one toggle, Close its icon, dividers go, so nothing clips; at < 760 pt the size shows "393 × 852".
- `app_state.canvases[].view.preview_state` reports size, css, rotated, zoom, device_frame,
  custom_open, placed. Web op actions `preview`, `preview_set`, `preview_custom`, `close_preview`
  drive it in e2e.
- Tests: unit `preview_sizes_fit_under_its_bar`, `preview_zoom_and_device_frame`; e2e
  `screens-panels.test.js` (fit, frame, 100% at scale 1, desktop, custom error then 1024 × 768,
  narrow 900 × 700 window).

## 4. B15: every button checked — done (audit updated)

`docs/notes/2026-11-07-canvas-controls.md` updated for this pass: screen bar, Preview's controls
one by one, both panels, Prototype tab. Fixed along the way: the screen panel's inapplicable
sections and fields, Preview's clipped bar at narrow widths, the custom-size error under the page,
the size menu not spanning its row, converted frames named "Frame". Not verified: Open in
Browser end to end (file URL in the Browser tab); version history (as before).

## 5. B12 leftovers — done

- `canvas_export` answers the design lint's summary of the page (errors, warnings, first five
  findings) by default; `lint: false` leaves it out. `canvas_screen_get` answers the design
  standard's machine checks of the screen's page (`design_checks`), as a write does.
- Lint fix: a fill bound with `fill: {"$var": …}` (cnv.15's own recipe) was reported as a raw
  colour (col.16); now it counts as bound.
- Starter fix: the starter's buttons were 43 px tall (an a11y.2 error in every design built from
  it) and its gaps 10 px (spc.1): buttons and the field now have a 44 px minimum and an 8 px gap
  bound to `space/2`; the Input has no grey fill of its own and its field fills its width. The
  starter test now asserts no lint errors and no off-scale gaps.
- **End-to-end "SaaS dashboard" run** (`docs/design/b12-saas-before.sawit`,
  `b12-saas-after.sawit`; driven through `sawit-mcp` only, app closed): "a SaaS dashboard,
  settings and login screens".
  - *Before* (old way: raw hex, hand-placed boxes, copies, placeholder copy): **11 errors, 13
    warnings** (col.10 × 8, cpy.19 × 3, cnv.11 × 7, cnv.15 × 3, cnv.12 × 2, cnv.13 × 1); workflow
    stage "no system".
  - *After* (canvas_starter_system → text properties on its components → a Nav item and a
    Sidebar component → Screens page with Dashboard, Settings and Sign in built from instances
    with auto layout and tokens → Cover page): **0 errors, 2 warnings** (cnv.11 on the starter's
    Components showcase frame, cmp.9 on the sign-in card's inputs); stage "cover", 21 tokens, 6
    components, 30 instances, 3 screens. PNG of the three screens checked by eye.

## Design review (better-interface, six domains) of the rendered result

**Scope:** what this track rendered in its own build, from e2e screenshots
(`e2e/artifacts/<run>/screens-*`): the Design panel on an HTML screen, on a converted text and on
a converted frame; the Layers panel (pages, Find, rows, the HTML row); the screen bar with Convert
to Layers; Preview at Fit, with the device frame, at 100%, at Desktop, Custom size with an error
and applied; a 900 × 700 window (Preview's bar and the screen panel); the converted frame beside
the HTML screen. Also the previous track's B15 work it touches (screen row tag, Prototype tab,
timer pill placement). Stack: GPUI, the app's theme tokens (zed-one-dark), gpui-component
controls, B16's `menu_icons` / `FloatBar`. Dark is the only theme. Conventions read: the backlog's
ground rules, `sawit-canvas-design`, the control audit.

| Domain | Evidence inspected | Result |
|---|---|---|
| Accessibility | names on every icon button (`accessibility_label`), fold buttons (keyboard-reachable `Button`s), disabled states with reasons, error with icon + text, contrast pairs below | 2 fixed, 1 open (LOW) |
| Layout | screen panel, Preview bar at 1200 and 900 wide, the size menu row, the error strip vs the page | 3 fixed (1 HIGH) |
| Writing | Screen section copy, toasts, tooltips, error text, button labels (verb first, sentence case per type) | 1 fixed, 1 open (LOW) |
| Typography | header truncation with tooltip, 12/13/14 pt sizes in the panels, tabular "393 × 852" labels | Clear |
| Colors | measured: primary label `#282c33` on `#74ade8` 5.93:1; muted text `#a9afbc` on `#282c33` 6.37:1 and on `#2e343e` 5.69:1; danger icon is paired with text | Clear |
| UI | bezel and shadow, inset rounded rows, icon set (one set, `menu_icons`), radii 6 px rows | 1 open (LOW) |

| Severity | Domain | Location | Before | After | Status |
|---|---|---|---|---|---|
| HIGH | Layout | `screens/preview_bar.rs` (bar) | At a 900 × 700 window the bar clipped Device frame, Reload, Open in Browser and **Close** off its right edge | Under 600 pt: zoom is one toggle, Close its icon (named "Close Preview"), dividers go; while typing a custom size only its fields and Close show | Fixed, rechecked in `preview-narrow.png` |
| HIGH | Layout / Accessibility | `screens/preview.rs` `place`, `preview_bar.rs` note | The custom-size error sat under the bar where the native page view covered it | A 36 pt strip under the bar is left free while an error shows; the error is an alert icon + text | Fixed |
| HIGH | Accessibility (B12, design output) | `ops/starter.rs` | Starter buttons 43 px tall: an a11y.2 error in every design built on it | 44 px minimum height and width | Fixed (unit test asserts 0 lint errors) |
| MEDIUM | Layout / Writing | `design_panel/mod.rs`, `screen.rs` | A screen showed Fill, Auto layout, Resizing, Layout grid (open MEDIUM from the canvas track), then rotation and align | Only X, Y, W, H and the Screen section | Fixed |
| MEDIUM | Writing | `layers_panel/rows.rs` HTML tag (canvas track's open MEDIUM) | What a screen is only in a mouse tooltip | Also said in visible text in the Design panel's Screen section; the tooltip says how to edit | Fixed |
| MEDIUM | Layout | `design_panel/screen.rs` size menu | The size menu hugged its label while the rows under it span the panel | Spans its row | Fixed |
| MEDIUM | Writing (cnv.8) | `screens/layers/looks.rs` | Converted frames without a name were "Frame" | Named by role: Header, Navigation, Button, List item, Row, Column, Box, Group | Fixed |
| MEDIUM | Colors (B12 lint) | `checks/lint.rs` | A fill bound with `fill: {"$var"}` reported as a raw colour | Counted as bound | Fixed |
| LOW | UI | `preview_bar.rs` bezel | Bezel colours `0x0e1013` / `0x3b414d` are literals, not theme tokens (a device's own colour, not the app's) | – | Open |
| LOW | UI | device frame | The page's corners are square inside a rounded bezel (the native view isn't clipped) | – | Open |
| LOW | Writing | Prototype Device menu | "None" as the device's name reads as missing | "No device" | Open (the engine's own value) |
| LOW | Accessibility | Preview size menu label | Under 760 pt it shows "393 × 852" alone; the device's name is in its accessible name and tooltip only | – | Open |

**Verification run:** e2e `screens-layers` and `screens-panels` (screenshots above, by eye);
unit tests (sawit-board 834, sawit-mcp 212, sawit app 915); `cargo fmt --all --check`; clippy
on sawit-board, sawit-mcp, sawit (with tests) clean.
**Not verified:** VoiceOver reading of the new controls; a full Tab walk through the panels
(GPUI's focus order in the panel was not driven); 200 % text size (a native app setting the
review can't change); Open in Browser end to end.

**Verdict: Approve** (no HIGH left; four LOW open). All six domains inspected.

## Tests run

- `cargo test -p sawit-board` 834 pass; `-p sawit-mcp` 212 + integration pass; `-p sawit` 915
  pass (one terminal timing test, `a_one_off_tab_closes_when_done_and_its_output_stays`, failed
  once under load and passed on rerun; unrelated). `cargo fmt --all --check` clean; clippy clean on
  the touched crates with tests. Line limit: none of this track's files over 500 (the script still
  lists other tracks' files: accounts, browser, seo, assets.rs).
- e2e (`--filter 'screens|canvas|live-cursors'`, which also ran capture, covers, find,
  fullscreen, accounts): 47 pass, 2 fail — `focus` (agent-desktop "Window focus did not settle")
  and `frame code` (agent-desktop "Accessibility surface read was incomplete" while opening the
  Pages menu): both harness failures in agent-desktop, neither touches this track's code; rerun
  noted below. All four `screens` specs pass (Convert to Layers, the panels and Preview, Present,
  three screens whole and sharp), and the canvas toolbar/flyout/context bar specs.

## Decisions

- Convert to Layers builds a new frame beside the screen instead of replacing it, and renames the
  screen "<name> / HTML", so names stay unique and nothing is lost; one undo takes both back.
- In-place HTML editing (the backlog's alternative) is not built: the layers path, which the
  backlog recommends, is.
- Preview's 100% keeps the page's width at one CSS px a point and lets a taller page scroll inside
  the room (its viewport height becomes the room's); a page wider than the room can't be shown at
  100% without panning the native view under the bar, so 100% is disabled there with why.
- Fit never scales up (a phone shows at its size on a large window, as before).
- Section folding is remembered for the app session, per section kind (not per file).
- The B12 run drove `sawit-mcp` with the app closed (a throwaway HOME so it doesn't reach the
  user's running Sawit, an older build without these ops).
- Commits were staged hunk by hunk from one working tree; only the final tree was built and
  tested. The README's MCP row for `canvas_export`/`canvas_screen_get`'s lint additions went in
  with the first commit (one line holds the whole Canvas row).
- Rerun of `^(focus|frame code|live)`: 50 pass, 3 fail. `focus: an agent's browser and canvas
  work…` and `frame code: the HTML lays the artboard out…` now pass (the earlier failures were the
  harness). New failures are not this track's: `frame code: a selected artboard has Export HTML
  and Export React over it` waits for an "Export HTML" button the frame bar hasn't had since B16
  (backlog 816b6923 has an Export menu instead: pre-existing); `focus: the user typing in the
  page…` and `live cursors: four agents…` failed on the control socket answering `""` (socket
  closed without a reply under load, the issue the canvas track also saw).

## Not verified

- Open in Browser from Preview, end to end (a `file://` URL in the project's Browser tab).
- VoiceOver names and a full keyboard Tab walk through the new panel controls.
- Convert to Layers on pages with background images, `::before` content or remote images (counted
  and reported, not drawn), and on screens of another page than the one on show (refused with a
  clear message, by reading the code).
- Fold state across files (it is per app session).

## Follow-ups

- `frame-code.test.js` should look for the Export menu B16 put in place of Export HTML / React.
- Text runs with mixed styles inside one paragraph convert as one text with the paragraph's style.
- A converted page taller than its screen keeps its full height (the frame grows); a "keep the
  viewport and scroll" option could follow.

## Commits (branch `wt/canvas2`, in order)

1. c813d9a6 feat(canvas): Convert to Layers, and layers as UI's default (B15)
2. 1b8352af feat(canvas): panels fitted to the selection, a compact Layers panel (B15)
3. 5194548f feat(canvas): Preview zooms, frames, sizes to order and opens in the browser (B15)
4. 53ccc22a docs(canvas): the control audit after the B15 second pass
5. 6d09878d feat(canvas): the lint in exports and screen reads, and a starter with no lint errors (B12)
