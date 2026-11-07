# Canvas controls audit (B15)

Every control on the canvas, by family: what it does, whether it works on a frame of layers and
on an HTML screen, how it looks against the design standard, its MCP path (B4,
`sawit_board::mcp_paths`) and its status. Checked on 2026-11-07 in the `wt/canvas` build.

Status: **works**, **fixed** (in this pass), **looks off** (works, needs the panel redesign),
**not verified** (not exercised by hand in this pass), **missing**.

The engine's menu items, context-menu items, tools and keys are hundreds of ids; they are not
listed one by one here. `mcp_paths::tests::every_canvas_function_has_an_mcp_path` lists them from
the menus, the toolbar, the flyouts and the keymap, and fails when one has no MCP path. Their
behaviour is FigCraft's own, covered by `crates/board` tests and the canvas e2e specs.

## Top bar

| Control | Does | Layer frame | Screen | Look | MCP path | Status |
|---|---|---|---|---|---|---|
| Main menu (☰) | the engine's menus + Sawit's (Export HTML/React, Variables…, Comments…, Start Timer, Start Vote) | yes | yes | ok | `canvas_command`, table in `mcp_paths` | works |
| Canvas name | names the file | – | – | ok | `canvas_tabs` | works |
| Zoom menu | zoom commands with keys | – | – | ok | user only (the view) | works |
| Layers / Design toggles | show the panels | – | – | ok | user only (`view.*`) | works |
| Agents pill ("3 building") | who works now; opens the roster | – | – | ok | `app_state.agents` | works |
| Agent cursors toggle | shows or hides agents' cursors | – | – | ok | `canvas_settings` | works |
| Hide UI, Maximize | chrome off, canvas fills the window | – | – | ok | user only / `app_maximize` | works |

## Toolbar and flyouts

| Control | Does | Layer frame | Screen | Look | MCP path | Status |
|---|---|---|---|---|---|---|
| Tools (move, hand, frame, shapes, pens, text, stickies, connector, stamp, …) | pick the tool | yes | – | ok | `canvas_command tool.*`, or draw with `canvas_add_elements` | works (e2e `canvas-tools`) |
| Flyouts (^) | a group's tools, colours, shapes, styles, stamps | yes | – | ok | `tool.* {param}` | works (e2e) |
| Templates & table | place a template or a table | yes | – | ok | `canvas_command` | works |
| Follow agent (new, B4.1) | the view follows agents' changes; off after a pan or zoom | – | – | icon toggle, shown while an agent works | user only; read in `app_state.canvases[].view.follow` | works (e2e `live-cursors`, unit tests) |

## Floating bars

| Control | Does | Layer frame | Screen | Look | MCP path | Status |
|---|---|---|---|---|---|---|
| Context bar | the selection's quick properties | yes | frame only | ok | `canvas_update`, `node.set` | works |
| Screen bar: Preview, downloads, Send to Agent | per screen | – | yes | B13/B16 own the redesign | `canvas_screen_get`, `canvas_screen_export`, `canvas_handoff` | works; redesign is B13 |
| Frame bar: Export HTML / React, Send | per artboard | yes | – | B13/B16 | `canvas_screen_export {id}`, `canvas_handoff` | works |
| Route bar: Live, Reload, Full page, Open in Browser | per route frame | – | route | ok | `canvas_add_routes` | works (e2e `covers`) |
| Timer and vote pill | the board's countdown and vote | – | – | was always shown over the top right | `board.timer`, `board.vote` | **fixed**: shows only while one runs or has results, at the bottom right above the toolbar, never over Preview or Present; started from View › Start Timer / Start Vote |
| "Loading…" pill on a screen | a screen with no good picture yet says so | – | yes | ok | `canvas_screen_get` waits for it | **fixed** (B17) |

## Preview

| Control | Does | Look | MCP path | Status |
|---|---|---|---|---|
| Bar | name, size picker, Rotate, Reload, Close | was a scrolling row of every size, cut off and covered by the timer pill | `canvas_screen_get` | **fixed**: one size menu (B16's bar, with icons) whose devices are grouped Mobile, Tablet, Laptop, Desktop under headings (B15); the name truncates with its full text in a tooltip; nothing is drawn over it |
| Rotate | turns the size | ok | – | works |
| Reload (new) | reloads the page | ok | – | works |
| Zoom fit / 100 % | – | – | – | **missing**: Preview always fits |
| Device frame | – | – | – | **missing** |
| Open in Browser | – | – | – | **missing** for screens (they are served from a private scheme) |
| Custom size | – | – | – | **missing** (waits for D1's sizes) |

## Present

| Control | Does | Look | MCP path | Status |
|---|---|---|---|---|
| Stage | draws the frame on show | – | user only; `app_state.canvases[].view.presenting` | **fixed**: a screen (or screens inside the frame) is drawn from its page's picture, taken again at 2× while Present shows, instead of a blank white frame |
| Back, Close | go back, close | now with icons and key hints | – | works |
| Flows between screens | – | – | – | **not verified**: HTML screens have no prototype links |

## Left panel (Layers, Assets) and right panel (Design, Prototype, Inspect)

| Control | Status |
|---|---|
| Pages list, Find layers, layer rows, rename, drag, visibility and lock | works; icons by kind were there; **fixed**: a sticky, shape or label is called by its first words, not "Sticky"; **not done**: compact pages, search in the header, 28 px rows |
| A screen in the Layers panel | **fixed**: its own icon (app window) and an "HTML" tag whose tooltip says its content is a web page; route frames show a globe |
| Design panel sections (Position, Layout, Fill, Stroke, Effects, Typography, Component, Layout grid) | works on layers; **looks off** on a screen (shows Fill, Auto layout and the rest, which a screen's HTML covers) |
| Prototype tab: Present, + Interaction, + Flow | **fixed**: Present full width with a play icon and its key; Interactions and Flows sections with icons and empty states; add buttons with a + icon; flow rows with icon buttons. **Not done**: prototype settings section |
| Inspect (Dev Mode) | works |

## Windows and dialogs

| Control | MCP path | Status |
|---|---|---|
| Variables… | `variable.*` | works |
| Comments… | `comment.*`, `canvas_get.comments` | works |
| Version history | `version.*` | not verified |
| Add Routes from Dev Server… | `canvas_add_routes` | works (e2e) |
| Actions palette, shortcuts sheet | – (the engine's commands) | works |

## Not done in this pass (B15)

- Layers as the source of truth for UI, Convert to Layers, and in-place editing of HTML screens:
  not started. Agents can already build editable UI from layers with the new starter system and
  lint (B12), but `canvas_begin`'s UI default is still `kind: "screen"`.
- The right panel fitted to the selection (a screen getting only name, size, position, Preview,
  Export), compact two-column fields and collapsible sections; the left panel's compact pages and
  search in its header.
