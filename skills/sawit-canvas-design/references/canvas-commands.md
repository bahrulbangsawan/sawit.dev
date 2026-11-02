# Canvas commands for design work

The commands `SKILL.md` names, with the parameters the engine documents for them. They come from the command registry of FigCraft, the canvas engine Sawit pins (`https://github.com/storytold/figcraft`, rev `0971d096db18aa107c5f3d1f83af4ac7f89ea2c3`; engine paths below are relative to `crates/engine/src/`), and from Sawit's canvas code (paths relative to Sawit's repository). Nothing here was run against a live canvas: the parameter shapes were read from the source. `canvas_commands {filter: "layout."}` prints the live list with parameters; trust it over this page if they differ.

The engine registers 621 commands (`cmd.rs:63-129`). `canvas_commands` offers 558 of them: Sawit hides the ones it doesn't draw or does itself (`crates/board/src/policy.rs:12-71`, `crates/board/src/ops/refusal.rs:17-30`, `crates/board/src/ops/command.rs:16-33`). Both counts were taken from the source, not from a live call.

## Which tool for what

| Job | Tool | Why |
| --- | --- | --- |
| Screens, pages, components, design systems | `canvas_command` and `command.batch` | the only route to frames, auto layout, components, variables and styles |
| Sticky notes, quick sketches, flow boxes on a board | `canvas_add_elements` | its `frame` becomes a section and its shapes become board shapes, sticky yellow `#FFE78A` without a `backgroundColor` (`crates/board/src/skeleton/layers.rs:61-78`, `:124-133`; `crates/mcp/src/app/canvas/args.rs:25`) |
| Diagrams (flowchart, sequence, ER, architecture, C4, ...) | `canvas_add_diagram` | Mermaid laid out as editable shapes (`crates/mcp/src/app/canvas/mod.rs:32-52`) |
| Small edits by id (position, size, colors, text) | `canvas_update` | `crates/mcp/src/app/canvas/mod.rs:100` |
| Read the page | `canvas_get`, then `page.list`, `node.get`, `node.find` | `canvas_get` lists only the top-level layers of the page on show (`crates/mcp/src/app/canvas/mod.rs:54-56`) |
| See the result | `file.export`, `canvas_export` | `canvas_export` writes the page on show at 1× as SVG or PNG (`crates/mcp/src/app/canvas/mod.rs:180`) |

Board or design file: a canvas file is one or the other (`crates/board/src/doc/mod.rs:20-26`). Frames, auto layout, components, variables and styles work in both. `proto.*` and `dev.*` are refused on a board (`boards/board.rs:209-217`); `board.*` tools are refused in a design file (`boards/board.rs:204-207`), and there `canvas_add_elements` draws only text, frames and images (`crates/board/src/ops/elements.rs:67-69`). The project canvas `.nyawit/canvas.sawit` is a board (`README.md:500-502`); a `.sawit` path given as `file` that doesn't exist yet starts as a new board (`crates/mcp/src/app/canvas/args.rs:7`). Sawit makes no new design file (`crates/board/src/doc/error.rs:30-33`).

With the app closed, every call starts from the saved file, so nothing carries over between calls: no selection, no undo, no view. Pass ids, or run the steps in one `command.batch` (`crates/mcp/src/lib.rs`, `INSTRUCTIONS`, the `canvas_command` line). Deleting commands keep the board beside the file as `<file>.del.bak` first (`crates/mcp/src/app/canvas/mod.rs:245`).

## Create

| Command | Parameters | Notes | Source |
| --- | --- | --- | --- |
| `frame.create` | `{preset?, x?, y?, width?, height?, parent?, index?, name?, props?, select?}` | presets in `references/tokens.md`; `props` takes any `node.set` key, so a frame can get its auto layout and fill in the same call | `cmd/create.rs:62-66`, `:368-392` |
| `frame.presets` | `{}` | lists the presets | `cmd/create.rs:125` |
| `frame.swapOrientation` | `{ids?}` | portrait to landscape | `cmd/create.rs:116` |
| `node.create` | `{type: rectangle, ellipse, polygon, star, line, arrow, vector, text, frame, section, component, slice; x?, y?, width?, height?, parent?, index?, name?, text?, path?, points?, props?, select?}` | frames default to 100×100, sections to 400×300; sections go only on the page or in sections | `cmd/create.rs:55-59`, `:263-286` |
| `text.create` | `{text, x, y, parent?, index?, name?, width?, props?, select?}` | with a `width` it wraps and hugs its height; the name defaults to the first line | `cmd/create.rs:95-99`, `:317-335` |
| `shape.rectangle`, `shape.ellipse`, `shape.line` | `{x, y, width, height, parent?, index?, name?, props?, select?}` | | `cmd/create.rs:69` |
| `image.place` | `{data (base64) or path, x?, y?, width?, height?, parent?, name?, target?, mode?: fill, fit, crop, tile}` | | `cmd/create.rs:109` |
| `edit.duplicate` | `{ids?, dx?, dy?}` | top-level frames land 100 to the right; a main component's copy is an instance | `cmd/edit.rs:111` |

Defaults to override every time: layers are named `Frame 1`, `Section 2`, `Component 3` and so on (`crates/doc/src/lib.rs:1009-1030`); new text is Inter Regular 12 with auto line height (`crates/doc/src/text.rs:273-293`); in the app, text with no fill is drawn readable on its page, white on a dark one (`cmd/create.rs:336-342`).

## Auto layout

| Command | Parameters | Notes | Source |
| --- | --- | --- | --- |
| `layout.add` | `{ids?, direction?: horizontal, vertical, grid}` | infers gap and padding from positions; layers that aren't all frames are wrapped in a new frame with no fill or clipping that hugs them | `cmd/structure.rs:73`, `:612-656` |
| `layout.suggest` | same | the same command under the Suggest name | `cmd/structure.rs:86` |
| `layout.remove` | `{ids?}` | | `cmd/structure.rs:74` |
| `layout.setTrack` | `{ids?, axis: columns or rows, index, sizing: fixed, hug, fill, value?}` | grid tracks | `cmd/layout.rs:12-19` |
| `node.set` | `{id or ids, props}` | layout keys: `layout`, `gap`, `crossGap`, `padding` (n or [t, r, b, l]), `paddingTop/Right/Bottom/Left`, `mainAlign` (min, center, max, spaceBetween, spaceAround, spaceEvenly), `crossAlign` (min, center, max, baseline), `wrap`, `gridColumns`, `gridRows`, `columnTracks`, `rowTracks`, `gridColumn`, `gridRow`, spans, `sizingH`/`sizingV` (fixed, hug, fill), `absolute`, `minWidth`, `maxWidth`, `minHeight`, `maxHeight`, `constraints` | `cmd/edit.rs:307`, `cmd/props.rs:19-37` |
| `measure.padding` | `{id?}` | the padding and gaps of an auto layout frame, to check | `canvas/distance.rs:400` |

Layout grids (`grid.*`) and ruler guides (`guide.*`) exist in the engine, but Sawit's canvas doesn't draw them (`crates/board/src/overlay/mod.rs:11-17`; `crates/board/src/policy.rs:44-51`), so a design must not depend on them.

## Components and instances

| Command | Parameters | Notes | Source |
| --- | --- | --- | --- |
| `component.create` | `{ids?}` | one frame or group becomes a component in place, keeping its id; several layers are wrapped first | `cmd/component.rs:58-64`, `:315-351` |
| `component.combineVariants` | `{ids?}` | wraps main components in a new set and returns its id; names without `=` become `Property 1=Default`, `Property 1=Variant2` | `cmd/component.rs:75`, `cmd/component/variants.rs:71-95` |
| `component.addVariant` | `{id?, property?, value?}` | duplicates a variant 20 below the lowest one and returns the new variant's id; `property` must already exist | `cmd/component.rs:82`, `cmd/component/variants.rs:114-176` |
| `component.addProperty` | `{id?, name, type: boolean, text, instanceSwap, variant, slot; default?, layer?, preferred?}` | `layer` is bound to the new property | `cmd/component.rs:233` |
| `component.bindProperty` | `{layer?, property, field?: visible, characters, mainComponent, slot}` | binds a further layer | `cmd/component.rs:248` |
| `component.makeSlot` | `{id?, name?, preferred?}` | a frame inside a main component that instances fill freely | `cmd/component.rs:144` |
| `instance.create` | `{component, x?, y?, parent?}` | a set gives its default variant | `cmd/component.rs:89-102` |
| `instance.place` | `{component, x, y}` | centred on a page point; inserts into an auto layout frame by position | `cmd/component.rs:104` |
| `instance.setProperties` | `{ids?, properties: {name: value}}` | variant properties switch variants | `cmd/component.rs:226` |
| `instance.swap` | `{ids?, component}` | | `cmd/component.rs:218` |
| `component.list` | `{}` | components with their instance counts and Assets folders | `cmd/component.rs:259-262` |
| `component.detach` | `{ids?}` | avoid (cnv.14) | `cmd/component.rs:179` |

Variant names are `Property=Value` pairs joined by `, ` (`assets/components/variants.rs:28-46`); every combination must be unique (`variants.rs:17-27`); the set's default variant is the one nearest its top-left corner (`variants.rs:3-15`). A slash in a component's name makes Assets folders (`assets/library.rs:68`). Name variant components without a path (`State=Default`), because the text before `=` becomes the property name whole (`variants.rs:28-36`), then rename the set with `object.rename`.

## Tokens: variables and styles

| Command | Parameters | Source |
| --- | --- | --- |
| `variable.createCollection` | `{name, modes?: [names]}` | `cmd/styles.rs:115` |
| `variable.create` | `{collection, name, type: color, number, string, boolean; value or values: {modeName: value}, description?}` | `cmd/styles.rs:164` |
| `variable.addMode` | `{collection, name, from?}` | `cmd/styles.rs:140` |
| `variable.bind` | `{ids?, field (fill, stroke, cornerRadius, gap, paddingHorizontal, fontSize, ...), variable}` | `cmd/styles.rs:196` |
| `variable.setMode` | `{ids?, collection, mode}`, on frames or the page | `cmd/styles.rs:203` |
| `variable.export` | `{collections?, format?: json, css, tailwind, dart; path?}` | `cmd/variables_io.rs:18` |
| `style.create` | `{type: paint, text, effect, grid; name, from?, description?, value?}` | `cmd/styles.rs:31` |
| `style.apply` | `{ids?, style, target?, start?, end?}` | `cmd/styles.rs:38` |
| `paint.documentColors` | `{page?, limit?}`: solid colors on the page, most used first | `cmd/styles.rs:18` |
| `file.fontReport` | `{}`: fonts used by text layers | `cmd/file.rs:90` |

A fill or stroke takes a variable directly in `node.set`: `fill: {"$var": "bg/surface"}` (`cmd/props.rs:20`).

## Pages, sections, naming, arranging

| Command | Parameters | Source |
| --- | --- | --- |
| `page.add` | `{name?}`; a name starting with a hyphen adds a divider; returns `{id}` | `cmd/page.rs:39-60` |
| `page.rename`, `page.move`, `page.select`, `page.list` | `{id?, name}`, `{id?, index}`, `{id or index}`, `{}` | `cmd/page.rs:61-150` |
| `object.wrapInSection` | `{ids?, name?}` | `cmd/structure.rs:45` |
| `object.moveToPage` | `{ids?, page}` | `cmd/structure.rs:87` |
| `object.rename` | `{ids?, name?}` | `cmd/edit.rs:254` |
| `object.batchRename` | `{ids?, find?, regex?, replace?, name?, start?}` with `$n` counters | `cmd/edit.rs:267` |
| `distribute.horizontal`, `distribute.vertical` | `{ids?, spacing?}` | `cmd/arrange.rs:20` |
| `align.top`, `align.left`, ... | `{ids?}` | `cmd/arrange.rs:17` |
| `node.reparent` | `{ids?, parent, index?}` | `cmd/edit.rs` (`node.reparent`) |
| `node.find` | `{root?, name?, type?, text?, component?, limit?}` returns `{ids, total}`; name and text match as case-insensitive substrings | `cmd/edit.rs:312-318` |
| `node.get` | `{id}`: the layer with its properties and one level of children | `cmd/edit.rs:308-311`, `automation/inspect.rs:8-40` |
| `data.fill` | `{id, rows: [{<layer name>: text, ...}], gap?, formats?, dir?, format?, scale?}`: copies of a frame filled by layer name | `content/datafill.rs:17` |

## Read back, export, hand off

| Command | Parameters | Source |
| --- | --- | --- |
| `inspect.properties` | `{id?, colorFormat?}`: layout, auto layout, typography, fills, strokes, effects with style and variable names | `dev/devinspect.rs:318` |
| `measure.distances` | `{ids?, to?}` | `canvas/distance.rs:393` |
| `file.export` | `{ids?, format?: png, jpg, svg, pdf, webp, ...; scale? or size?; path?}`; without `path` it returns base64; a `path` that is a canvas file is refused | `cmd/file.rs:118`; `crates/board/src/ops/refusal.rs:48-55` |
| `inspect.code` | `{id?, language?: css, html, react, vue, tailwind, swiftui, compose, flutter, ...; mobile? (tailwind: a mobile frame with the same layer names), unit?}` | `dev/codegen.rs:40` |
| `annotation.add` | `{id?, label? or text?, properties?: [...], category?}` | `dev/annotations.rs:247` |
| `proto.interaction.add` | `{id?, trigger?, action, transition?, durationMs?, easing? (incl. customBezier)}`; design files only | `cmd/proto.rs:16` |
| `node.set {props: {devStatus: "readyForDev"}}` | marks a top-level frame or section ready for handoff | `cmd/props.rs:615-628` |

## Batches

`command.batch {label?, onError?: "abort" | "continue", commands: [{command, params, as?}]}` runs as one undo step (`cmd/edit.rs:463-470`). In a later step's params, `{"$step": n}` stands for the `id` in step `n`'s result (zero-based) and `{"$ref": "name"}` for the `id` of the step that set `"as": "name"`; either takes a `field` (a key or slash path into that result) (`automation/batch.rs:1-6`). Sawit refuses a batch step that would write a file at a path taken from another step (`crates/board/src/ops/refusal.rs:74-98`).

## Worked example: a button component with two states

Not run; the parameters follow the specs above. It assumes a `Tokens` collection with color variables `accent/solid`, `accent/solid-hover` and `text/on-accent`, and a text style you replace `<UI family>` with.

```json
{"command": "command.batch", "params": {"label": "Button component", "commands": [
  {"command": "frame.create", "as": "default", "params": {"name": "State=Default", "x": 0, "y": 0,
    "props": {"layout": "horizontal", "padding": [8, 16, 8, 16], "gap": 8, "mainAlign": "center",
              "crossAlign": "center", "sizingH": "hug", "sizingV": "hug", "cornerRadius": 8,
              "fill": {"$var": "accent/solid"}}}},
  {"command": "text.create", "as": "label", "params": {"text": "Save draft", "x": 0, "y": 0,
    "parent": {"$ref": "default"}, "name": "Label",
    "props": {"fontFamily": "<UI family>", "fontSize": 14, "lineHeight": 20, "fill": {"$var": "text/on-accent"}}}},
  {"command": "frame.create", "as": "hover", "params": {"name": "State=Hover", "x": 0, "y": 60,
    "props": {"layout": "horizontal", "padding": [8, 16, 8, 16], "gap": 8, "mainAlign": "center",
              "crossAlign": "center", "sizingH": "hug", "sizingV": "hug", "cornerRadius": 8,
              "fill": {"$var": "accent/solid-hover"}}}},
  {"command": "text.create", "as": "label2", "params": {"text": "Save draft", "x": 0, "y": 0,
    "parent": {"$ref": "hover"}, "name": "Label",
    "props": {"fontFamily": "<UI family>", "fontSize": 14, "lineHeight": 20, "fill": {"$var": "text/on-accent"}}}},
  {"command": "component.create", "params": {"ids": [{"$ref": "default"}]}},
  {"command": "component.create", "params": {"ids": [{"$ref": "hover"}]}},
  {"command": "component.combineVariants", "as": "set", "params": {"ids": [{"$ref": "default"}, {"$ref": "hover"}]}},
  {"command": "object.rename", "params": {"ids": [{"$ref": "set"}], "name": "Button"}},
  {"command": "component.addProperty", "params": {"id": {"$ref": "set"}, "name": "Label", "type": "text", "layer": {"$ref": "label"}}},
  {"command": "component.bindProperty", "params": {"layer": {"$ref": "label2"}, "property": "Label", "field": "characters"}}
]}}
```

Then place it: `instance.create {component: <set id>, parent: <screen frame id>}` and `instance.setProperties {ids: [<instance id>], properties: {"Label": "Publish"}}`; check the label's 14/20 against the 32–36 in-app button height (cmp.15), and add the `Pressed`, `Focus`, `Disabled` and `Loading` variants the same way (sta.5).
