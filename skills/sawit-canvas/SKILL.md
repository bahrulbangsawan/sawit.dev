---
name: sawit-canvas
description: Designs, wireframes, mockups and diagrams on Sawit's canvas instead of another drawing tool or ASCII art. Use it when the user asks to design or sketch a screen, flow or component, lay out a wireframe, draw an architecture, sequence, state or entity diagram, or review what is on the canvas, through the sawit MCP server's canvas tools. Read canvas_design_guide before any UI work.
---

# Sawit's canvas

Each project has a canvas in Sawit (`.nyawit/canvas.sawit`): pages, frames, shapes, text, stickies and connectors. The canvas tools work with the app open or closed; closed, they edit the saved file.

## UI: read the guide first

Before you design, build, restyle or review any UI on the canvas, call `canvas_design_guide` and follow it; `section` (such as Typography) returns one section with the checklist. Run its checklist before you call a design done. The `sawit-canvas-design` skill, where it is installed, is the same text.

## UI is a screen

A screen, page or app view is a screen on the canvas: `canvas_begin {kind: "screen", name, frame, steps}`, then `canvas_screen_write` with its HTML (the whole page, or edits) and files. Each write renders on the canvas, and its answer lists the design standard's machine checks (contrast, target and text sizes, spacing, alt text, labels, focus, overflow), each failure with its rule and the fix: fix them before you finish with `canvas_progress {status: "done"}`. `canvas_screen_get` reads a screen back with a PNG; `canvas_screen_export` writes it as HTML or React. The first UI work of a session answers with the standard's digest; name parts in `steps` (Form, Table, States) to get their sections. Vector frames (`canvas_command`, `canvas_add_elements`) are for components, diagrams, sketches the user asked for and illustrations.

## Systems and lint

- `canvas_starter_system` lays out a starter design system (tokens as variables, Light and Dark, and Button, Input and Card components) so a design never starts from nothing; build screens from its components.
- `canvas_lint` checks a frame (`id`), a work's frame (`work`) or the page against the design standard: off-token values, copies that should be components, uneven spacing, placeholder copy, generic generated looks and the machine checks, each with its fix. Its `status` says where the file is in the workflow and the next step. Run it before you call a design done, and fix every error.

## Diagrams

- `canvas_add_diagram` draws Mermaid syntax (`source`: flowchart, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, architecture-beta, C4Context, gantt, mindmap and more) as shapes the user can edit; `x` and `y` place it. It lays out offline.
- `render_diagram` writes the same source to an SVG file instead, for docs; `schema_diagram` draws a database's tables.
- `canvas_add_mermaid` is its deprecated old name: use `canvas_add_diagram`.

## Drawing and editing

- `canvas_get` lists the layers of the page on show (ids, positions, text). Read it before you change anything.
- `canvas_add_elements` adds rectangles, ellipses, diamonds, stickies, text, arrows bound to shapes by id, lines and frames.
- To build a component or vector frame, `canvas_begin` it first (`name`, `frame`, `steps`): pass the `work` it answers to the tools you build it with, say each `step` with `canvas_progress`, and end with `status` done or failed, so the user watches it being built.
- Orchestrating several agents (subagents, a workflow)? Give each its own stable `agent` id (`{id: "header", name: "Header agent"}`) and have it pass that on every canvas call: each then has its own cursor, colour and name on the canvas.
- `canvas_update` changes layers by id; `canvas_delete` removes them (the board is kept as a `.del.bak` first).
- `canvas_command` runs any engine command by id (board.sticky, align.left, node.set, page.list); `canvas_commands` with a `filter` lists the ids and what each takes. Commands that would move the user's view or page (view.*, page.select) are refused: address pages and layers by id. With the app closed each call starts from the saved file: pass ids, or put the steps in one command.batch.
- `canvas_export` writes the page on show as SVG or PNG.
- `file` works on another `.sawit` file (a design file) or an `.excalidraw` file instead of the project's canvas.
- `canvas_clear` and `canvas_load` replace a whole page or canvas (a `.bak` is kept): only when asked.

## Canvases, routes, settings and handoff

- `canvas_tabs` lists the project's canvases (each a `.sawit` file in `.nyawit/` with a tab of its own), and opens (a new one by `tab` name), closes, activates or renames one. `show` brings it forward; without it the canvas opens behind the user's tab.
- `canvas_add_routes` lays a dev server's routes out as live frames, a row per route and a frame per viewport (`url`, `routes`, `viewports`), each showing the page as WebKit renders it.
- `canvas_settings` reads or changes how builds play (`build_animation` full, fast or off; `sound`; `subagent_cursors`): turn the animation down for a long batch, and only when the user agrees, since they are the user's View settings.
- `canvas_handoff` hands frames or screens to code (`id`, `work` or `screen`): it writes the files and a prompt, and with `agent` (claude-code, codex, gemini, opencode) starts that agent in a new terminal tab with the prompt.

Show the canvas to the user with `app_open_page` (`page`: canvas).

## Without the sawit MCP server

Tell the user, as the `sawit` skill says. Meanwhile write a diagram as Mermaid in a Markdown file, or describe the design in words, and say it can go on Sawit's canvas once the server is connected.
