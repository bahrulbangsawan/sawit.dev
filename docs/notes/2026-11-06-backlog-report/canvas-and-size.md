# Backlog report, part 5: canvas to code, live cursors, the FigCraft gaps work, and size (items 27, 25, 26, 30, 7)

Back to the [summary](../2026-11-06-backlog-report.md).

---

## Item 27: the design standard is on by default (`38c3cce`)

**Asked:** an agent that never calls `canvas_design_guide` still gets the rules and has its work
checked.

**Built:**
- **A digest made at build time.** `crates/core/build.rs` writes `design_digest.md` from
  `skills/sawit-canvas-design/SKILL.md`: the floors line, the rules Sawit checks, the whole
  checklist and the list of sections (~7 KB, ~1.8k tokens). If SKILL.md loses a rule the digest
  names, the build fails.
- **`canvas_begin`** sends the digest with a session's first UI work (a screen, a screen-sized
  frame or a UI part). Later works get at most two sections their name or steps call for, each once
  per session. A screen-sized vector frame gets `use_screen`, telling the agent to use a screen.
- **Screens are the default for UI:** one line in `INSTRUCTIONS` (still exactly 4,000 characters),
  the `canvas_begin` description, rules cnv.1 and cnv.3, and `skills/sawit-canvas/SKILL.md`.
- **Machine checks** (`sawit_board::checks`): a static HTML+CSS checker (html5ever plus a small
  cascade) and a layer checker for artboards. Rules: contrast (contrast.py's formula in Rust),
  target size, minimum text size, spacing on the scale, missing alt text and nameless icon controls,
  fields without labels, no focus style, text that can't fit its box. They run after every
  `canvas_screen_write` and on `canvas_progress {status: "done"}`, and answer
  `design_checks: {passed, summary, failures: [{rule, selector|layer, fix}]}`.
- **Badge:** "N design issues" on the frame's corner, opened by the user, never by the agent.
- **MCP resources and prompt:** `sawit://design-guide`, `sawit://design-guide/<section>`, `design-ui`.

**Checked:** a test that the digest matches SKILL.md; a test that seeds a phone screen with a
low-contrast note, 10 px text and a 30 px button and gets exactly col.10, typ.11 and a11y.2; MCP
tests (digest once, sections once, `use_screen`, checks after writes, resources, prompt). e2e
`design checks: …` 2/2 pass.

**Not done:**
- The checks are static, with no layout engine: overflow and target widths are estimates (0.5 em
  per glyph), and contrast over images and gradients is skipped. A WebKit-side audit would be exact.
- The "fresh agent session builds a settings page and ends with no failures" run needs a real agent
  session. It was not run.

---

## Item 25: Export HTML and Export React for artboards (`5d45dc0`)

**Built:**
- `sawit_board::frame_code` walks a frame's layer tree using each layer's Copy-as-CSS declarations
  (no second style mapper), plus placement: auto layout becomes flexbox, other children are
  absolute. Text keeps its type; image fills become data URLs; vectors become inline SVG; bound
  variables become `:root` custom properties; layer names become classes and `data-name`. HTML is
  one file; React is `<Name>.tsx` plus `<Name>.module.css`; instances become React components with
  a string-union prop per variant property. Each file starts with a "mechanical reference" note.
- **UI:** a new artboard bar (Export HTML / Export React); the screen bar's Download is renamed
  Export; right-click and File ▸ Export entries; two buttons in the design panel's Export section
  (FigCraft's `ExportFormat` enum can't hold HTML). All ask where to save, starting in the project.
- **MCP:** `canvas_screen_export {id}` also exports an artboard by id or name.
- **`inspect.code`:** it does exist (FigCraft's, through `canvas_command`, html/react/tailwind). A test
  runs it; rule cnv.3 keeps it and now also names the exports.

**Checked:** unit tests (auto layout and text styles, absolute children, embedded image, an instance
with variants and a text override, menus, MCP export with the app closed). The `.tsx` type-checks in
a fresh Vite + React + TS app (`npx tsc --noEmit`, exit 0). e2e `frame code: …` 2/2 pass: every
layer of the exported HTML sits within 2 px of the canvas's boxes. That was a geometry comparison
from the page's DOM, not a pixel diff.

**Not done:** mixed text runs use the base style; rotations are approximate; masks and blend groups
are not special-cased; Tailwind comes only from `inspect.code`, not as a file export.

---

## Item 26: Send to Agent for artboards and flows (`69470e4`)

**Built:**
- `sawit_board::handoff` writes `.nyawit/handoff/<name>/`: `design.png` (@2×), per-frame PNGs for a
  flow, each frame's HTML and React, `spec.md` (sizes, layer tree, tokens, components and variants,
  copy, states drawn, prototype links) and `prompt.md` (build in the project's stack and components,
  follow the standard, check against `design.png` in Sawit's browser). Flows are in canvas order.
- **App:** Send to Agent… on the artboard bar, Send Flow to Agent… over several frames, a Send to
  Agent submenu in the right-click menu (only agents found on this Mac), Copy Prompt, Reveal Handoff
  Folder, a toast with the folder.
- **MCP:** `canvas_handoff {id | work | screen, agent?}`. With `agent` and the app running, it opens
  the agent in a new terminal tab.

**Checked:** unit tests of the folder, spec and prompt for one artboard, one screen and a two-frame
flow. e2e `handoff: …`: the Copy Prompt test passed. The agent test uses a stand-in `claude` in the
temp home; its last run in the canvas-code track was queued and its result is not in the report.

**Not done:** the agent actually building a page that matches `design.png` needs a signed-in coding
agent and the user.

---

## Item 30: live build cursors (`b755936`, `38e4c35`, `68014a4`, `d905cfb`, `e45e8ca`)

**Wrong (30a):** an identity was one `sawit-mcp` process, so Claude Code's subagents shared one id,
one colour and one pointer that jumped between their works.

**Built:**
- **30a** (`b755936`): `AgentIdentity` gains `lane {id, name?, parent?}`, keyed `session/lane`, each
  with its own colour, pointer, pill, rows and announcements. Lanes come from the `agent` argument
  (now on every drawing canvas tool), else from a session's second and later open works, else the
  session. The roster and edge chips group a session ("Claude Code · 4 agents").
- **30b** (`38e4c35`): Sawit's own vector pointer set (arrow, crosshair, I-beam, pen, ⌥-copy, hand)
  with a white edge and shadow; curved, eased trips of 120–450 ms; a click dip; "· typing" in the
  pill. Painted marks only.
- **30c and 30d** (`68014a4`): the document changes first; a drawn copy hides unplaced layers and is
  rebuilt only at milestones. Each op becomes a script in a designer's order (parent before child,
  master before copy, reading order), with budgets (900 ms per element, 2.5 s per op), a queue per
  lane playing at once, catch-up 2× → 4× → snap, and snaps on undo, edit or select. Animations for
  frames, shapes, stickies, components, auto-layout spacing, text, images, lines, duplicates with
  distance marks, moves, resizes and deletions. `reveal.rs` is replaced.
- **30e** (`d905cfb`): a blueprint script in the screen's web view after each write; only the diff
  plays, and each element's cover fades to the live page.
- **30f** (`e45e8ca`): View menu Build Animation Full / Fast (2×) / Off, Build Sounds (off), Show
  Subagents as Separate Cursors (on); MCP `canvas_settings`; reduced motion = jumps and 150 ms
  fades; sounds synthesised at runtime and played by NSSound. Only an objc2-app-kit feature was
  added, no new crate. `CREDITS.md` credits swarm-cursors and Swarm as inspiration; no code, art or
  timings were copied.

**Decisions:** lanes from works only when a session has more than one open work; MCP `_meta` isn't
used, because no client's stable subagent id could be verified; 30c and 30d are one commit; the
settings live in the canvas's View menu (there is no canvas Settings page); a screen's "masking" is
per-element white covers over the live page.

**Checked:** unit tests (4 lanes → 4 cursors, colours and names; 3 open works → 3 cursors; trip
curves; script order; budgets and catch-up; 4 lanes playing at once; undo cancelling a replay; Fast,
Off and reduced motion; the blueprint diff; sounds). e2e: all 3 `live cursors` cases pass (four lanes
at once, selection and view untouched; a screen written three times plays three builds and ends
live). CPU on a debug build, % of one core: 115.4 before, 100.4 with four lanes playing, 101.7 idle
after.

**Not done:** screen captures; 60 fps and CPU on a release build with 6 lanes; a real Claude Code
session with subagents passing `agent`.

---

## The canvas FigCraft gaps work (another session's, integrated)

Another session had uncommitted canvas work on `main`. It was backed up first
(`sawit-wt/reports/preserve/`: `canvas-figcraft-gaps.patch` and an archive of its untracked
files), then committed in 9 commits on `wt/others` and integrated:

- `e9f7630` FigCraft bumped to v0.15.0 (a fill-distribution hang, quadratic slowdowns on large
  files, mask nesting and layer-blur bounds fixed upstream)
- `7918eb0` route frames show the whole page and follow their size
- `9fa722f` Prototype tab, Present player, Dev Mode's Inspect panel
- `1aa6b15` Variables window, component properties, Assets tab
- `508aee4` gradients, image fills, effect fields, layout and type details
- `48a9e5a` comments, canvas aids, Shape Builder, a board's timer and votes
- `8e01cc3` dialogs for commands that need a value, version history
- `51fb03a` the new panels, windows and tools wired into the canvas
- `b9437c6` the plan, `docs/notes/2026-11-06-figcraft-gaps.md`, with its status

**Gaps:**
- No e2e for its new UI.
- No MCP tools of its own. Agents reach it through `canvas_command` by engine command id, and the
  README now says how (`ace91bb`, which also makes `canvas_get` return comment threads).
- The timer pill shows on every board.
- The plan's own "Still open" list: live collaboration, branches, plugins and libraries; on-canvas
  handles (gradient, auto layout padding and gap, constraint guides, drop cells, Scale readout);
  vector width tools and the point panel; Dev Mode measurements, Compare changes, dev resources and
  Code Connect; prototype transitions other than instant, conditionals, scroll and overlay
  positions; comment @mentions, reactions and read state; `edit.copyLink` and the missing-fonts
  dialog; remote route frames.

---

## Item 7: size and memory

**Commit:** `d779962` only (a spec fix). **No size or memory change is committed.** The memory
savings below come from item 10 (only the tab on show keeps a live page).

**Method:** release builds (fat LTO, strip, codegen-units 1). "Before" is v0.0.9 (`3a3419f`) in a
throwaway worktree; "after" is backlog `bd148be`. RSS from `ps` on a scratch `SAWIT_HOME` through
the control socket. Idle is the welcome screen after 8 s. Busy is a terminal, the browser on a local
page and the canvas with a shape. The soak is 300 s: 99 rounds of `seq 1 20000` in the terminal, a
browser navigation and a canvas add.

| | v0.0.9 | backlog | Δ |
|---|---|---|---|
| `sawit` binary | 83.4 MB | 85.4 MB | +2.0 MB |
| Sawit.app | 84.2 MB | 86.0 MB | +1.8 MB |
| release zip | 35.2 MB | 36.0 MB | +0.8 MB |
| idle RSS | 119.4 MB | 113.1 MB | −6.3 MB |
| one project | 125.4 MB | 117.4 MB | −8.0 MB |
| busy, Sawit only | 307.4 MB | 297.3 MB | −10.1 MB |
| busy, Sawit + WebKit (GPU, WebContent, Networking) | 363.6 MB | 350.7 MB | −12.9 MB |
| after soak, Sawit only | 248.5 MB | 183.6 MB | −64.9 MB |
| after soak, Sawit + WebKit | 370.7 MB | 223.0 MB | −147.7 MB |

v0.0.9 is left with a second WebContent process (58.7 MB) after the soak; backlog has one.

**Browser tabs** (item 10, debug build, browser-e2e-14), Sawit + its pages:

| Live tabs | Sawit + pages |
|---|---|
| 1 | 189 + 35 MB |
| 3 | 190 + 93 MB |
| 6 | 189 + 180 MB |
| 6, with 5 suspended | 134 + 15 MB |

Each live page costs about 30 MB, and suspension gives that back.

**Where the size is:** the binary is 84.9 MB of the bundle (plus Sawit.icns 462 KB, NOTICE 25 KB).
Embedded assets are about 3.7 MB (4 %): mesurer.js 800 KB, pele.global.js 734 KB, Inter fonts
1.79 MB, skills 180 KB, SVG icons 187 KB. All are needed at run time.

**Memory:** vmmap footprint is 75 MB idle (peak 141 MB) and 170 MB with the canvas open (peak 446
MB; ~82 MB GPU surfaces, 48 MB large mallocs). The canvas peak is figcraft-text reading system font
files into memory, upstream and out of scope. Terminal scrollback stays at 10,000 lines; the soak
shows the cap already holds memory down.

**Duplicates:** `cargo tree -d` lists 104 duplicated crates in both v0.0.9 and backlog, all
transitive (gpui, wry, figcraft, font crates). None can be removed from Sawit's own manifests.
`cargo bloat --crates` needs another multi-hour unstripped build, so the largest rlibs were used as a
proxy (objc2_app_kit, gpui_component, objc2_foundation, sawit_core, gpui_base, figcraft engine,
format and collab, rmcp, fonts, lsp_types, image, krilla, and wasmi at 17 MB through
figcraft-plugin).

**Binary growth:** +2.0 MB for 30 items. New dependencies stated by their tracks: vendored
gpui-component 0.8.1 (~4 MB of source, a fork of a crate already used), an objc2-app-kit feature
(item 30), objc2-foundation features (item 21), `sha1` already present (item 20).

**Follow-up, untested:** `opt-level = "s"` for cold crates: wasmi, wasmi_core, wasmi_ir, wasmparser,
figcraft-plugin, figcraft-collab, krilla, rmcp, rquickjs, rquickjs-core, rquickjs-sys, moxcms,
object, gimli, addr2line. It was dropped untested, because measuring it takes two release builds of
the same commit (~7 h together at this machine's load). Try it on a quiet machine, and keep it only
if the binary shrinks by 1 MB or more with no slower canvas, plugins or MCP.

**Not done:** the item asks for a 30-minute soak; the soak was 300 s. The memory numbers were taken
on a busy machine and should be measured again on a quiet one.
