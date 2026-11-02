---
name: sawit-canvas
description: Designs, wireframes, mockups and diagrams on Sawit's canvas instead of another drawing tool or ASCII art. Use it when the user asks to design or sketch a screen, flow or component, lay out a wireframe, draw an architecture, sequence, state or entity diagram, or review what is on the canvas, through the sawit MCP server's canvas tools. Read canvas_design_guide before any UI work.
---

# Sawit's canvas

Each project has a canvas in Sawit (`.nyawit/canvas.sawit`): pages, frames, shapes, text, stickies and connectors. The canvas tools work with the app open or closed; closed, they edit the saved file.

## UI: read the guide first

Before you design, build, restyle or review any UI on the canvas, call `canvas_design_guide` and follow it; `section` (such as Typography) returns one section with the checklist. Run its checklist before you call a design done. The `sawit-canvas-design` skill, where it is installed, is the same text.

## Diagrams

- `canvas_add_diagram` draws Mermaid syntax (`source`: flowchart, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, architecture-beta, C4Context, gantt, mindmap and more) as shapes the user can edit; `x` and `y` place it. It lays out offline.
- `render_diagram` writes the same source to an SVG file instead, for docs; `schema_diagram` draws a database's tables.
- `canvas_add_mermaid` is its deprecated old name: use `canvas_add_diagram`.

## Drawing and editing

- `canvas_get` lists the layers of the page on show (ids, positions, text). Read it before you change anything.
- `canvas_add_elements` adds rectangles, ellipses, diamonds, stickies, text, arrows bound to shapes by id, lines and frames.
- To build a component or screen, `canvas_begin` it first (`name`, `frame`, `steps`): pass the `work` it answers to the tools you build it with, say each `step` with `canvas_progress`, and end with `status` done or failed, so the user watches it being built.
- `canvas_update` changes layers by id; `canvas_delete` removes them (the board is kept as a `.del.bak` first).
- `canvas_command` runs any engine command by id (board.sticky, align.left, node.set, page.list); `canvas_commands` with a `filter` lists the ids and what each takes. Commands that would move the user's view or page (view.*, page.select) are refused: address pages and layers by id. With the app closed each call starts from the saved file: pass ids, or put the steps in one command.batch.
- `canvas_export` writes the page on show as SVG or PNG.
- `file` works on another `.sawit` file (a design file) or an `.excalidraw` file instead of the project's canvas.
- `canvas_clear` and `canvas_load` replace a whole page or canvas (a `.bak` is kept): only when asked.

Show the canvas to the user with `app_open_page` (`page`: canvas).

## Without the sawit MCP server

Tell the user, as the `sawit` skill says. Meanwhile write a diagram as Mermaid in a Markdown file, or describe the design in words, and say it can go on Sawit's canvas once the server is connected.
