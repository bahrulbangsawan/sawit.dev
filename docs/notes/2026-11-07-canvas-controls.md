# Canvas controls audit (B15)

Every control on the canvas, by family: what it does, whether it works on a frame of layers and
on an HTML screen, how it looks against the design standard, its MCP path (B4,
`sawit_board::mcp_paths`) and its status. Checked on 2026-11-07 in the `wt/canvas` build, and again in the `wt/canvas2` build (B15 second pass: Convert to Layers, the panels fitted to the selection, Preview's zoom, device frame, Open in Browser and custom size).

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
| Screen bar: Preview, Convert to Layers, Export, Send to Agent | per screen | – | yes | B16's bar, icons | `canvas_screen_get`, `canvas_screen_to_layers`, `canvas_screen_export`, `canvas_handoff` | works; **new**: Convert to Layers (e2e `screens-layers`) |
| Frame bar: Export HTML / React, Send | per artboard | yes | – | B13/B16 | `canvas_screen_export {id}`, `canvas_handoff` | works |
| Route bar: Live, Reload, Full page, Open in Browser | per route frame | – | route | ok | `canvas_add_routes` | works (e2e `covers`) |
| Timer and vote pill | the board's countdown and vote | – | – | was always shown over the top right | `board.timer`, `board.vote` | **fixed**: shows only while one runs or has results, at the bottom right above the toolbar, never over Preview or Present; started from View › Start Timer / Start Vote |
| "Loading…" pill on a screen | a screen with no good picture yet says so | – | yes | ok | `canvas_screen_get` waits for it | **fixed** (B17) |

## Preview

| Control | Does | Look | MCP path | Status |
|---|---|---|---|---|
| Bar | name (cut short, whole name in its tooltip), size menu, Rotate, Fit / 100%, Device frame, Reload, Open in Browser, Close | one row of icon buttons, no scrolling, nothing drawn over it; fits a 900 × 700 window | `canvas_screen_get`; `app_state.canvases[].view.preview_state` | **fixed** (canvas2) |
| Size menu | As Made, Window, D1's 20 devices grouped Mobile / Tablet / Laptop / Desktop, Custom size… | icons per kind, the size shown checked | – | works (e2e `screens-panels`) |
| Custom size | width and height typed in the bar (200–3840), Enter or Apply, Escape or × cancels; a wrong size says what to type | inline fields, error note under the bar | – | **new**, works (e2e: a wrong size keeps the fields and says why; 1024 × 768 lays the page out at it) |
| Rotate | turns the size | icon toggle, off for Window | – | works |
| Fit / 100% | fits the page in the room (never up) / one CSS px a point, a taller page scrolling inside the room | segmented pair; 100% is off with a tooltip when the window is narrower than the page | – | **new**, works (e2e: scale 1 at 100%) |
| Device frame | a bezel and shadow round a phone or tablet size | icon toggle, off (with why) for laptop and desktop sizes | – | **new**, works (e2e) |
| Reload | reloads the page | icon | – | works |
| Open in Browser | opens the screen's `index.html` in the project's Browser tab and closes Preview | icon | – | **new**; not verified end to end (file URL in the Browser tab) |

## Present

| Control | Does | Look | MCP path | Status |
|---|---|---|---|---|
| Stage | draws the frame on show | – | user only; `app_state.canvases[].view.presenting` | **fixed**: a screen (or screens inside the frame) is drawn from its page's picture, taken again at 2× while Present shows, instead of a blank white frame |
| Back, Close | go back, close | now with icons and key hints | – | works |
| Flows between screens | – | – | – | **not verified**: HTML screens have no prototype links |

## Left panel (Layers, Assets) and right panel (Design, Prototype, Inspect)

| Control | Status |
|---|---|
| Find (search field) | **moved** to the panel's head, under the Layers / Assets tabs, with its icon and clear button (canvas2) |
| Pages list | compact 28 px rows inset with rounded corners, a page icon, the page on show marked with a check; fold and add in its header; rename in place and the context menu as before |
| Layer rows | 28 px, inset and rounded, icons by kind, a sticky's first words, visibility and lock on hover, indentation and expand / collapse, the selection marked; works |
| A screen in the Layers panel | its own icon and an "HTML" tag whose tooltip says "HTML screen: its content is a web page. Select it and choose Convert to Layers to edit it here" |
| Design panel on a screen | **fixed** (canvas2): only Position and a Screen section (what it is, its size as a device, Convert to Layers, Preview, Export); no Fill, Auto layout, Resizing, Layout grid, Typography or Export settings |
| Design panel header | the kind ("Screen" for a screen), the name cut short with its whole name in a tooltip, an HTML tag for a screen |
| Design panel on text | Typography comes first, then Position and the rest (canvas2) |
| Section headers | each folds away or opens with a click on its title (a button with a chevron; keyboard reachable); a folded section is one row |
| Prototype tab: Present, Device, Interactions, Flows | **fixed**: a Device section at the top (the device Present shows the frames in, and Landscape for devices that turn), then Interactions and Flows with icons and empty states |
| Inspect (Dev Mode) | works |

## Windows and dialogs

| Control | MCP path | Status |
|---|---|---|
| Variables… | `variable.*` | works |
| Comments… | `comment.*`, `canvas_get.comments` | works |
| Version history | `version.*` | not verified |
| Add Routes from Dev Server… | `canvas_add_routes` | works (e2e) |
| Actions palette, shortcuts sheet | – (the engine's commands) | works |

## Not done (after the canvas2 pass)

- In-place editing of HTML screens (the backlog's alternative to Convert to Layers): not built;
  Convert to Layers is the path, as the backlog recommends.
- The design panel's header name is not editable in place (rename it in the Layers panel, ⌘R).
- Open in Browser for a screen: built, not verified end to end.
