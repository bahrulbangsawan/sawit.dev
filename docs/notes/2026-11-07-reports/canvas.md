# Track `canvas` report (B17, B15, B4, B4.1, B4.2, B12)

Worktree `/Volumes/bahrul/project/sawit-wt/canvas`, branch `wt/canvas`. Commits are listed at the
end, in order.

## B17: screens black, half-drawn, blurry

**Reproduced** with `e2e/specs/screens-settle.test.js` (three 393×852 screens written in a row:
Progress plain, Today with a 4 s staggered entrance and a clip-reveal heading, Session revealed by
`setTimeout`s). On the pre-fix build (`target-canvas/sawit-before`): Today stopped after its second
card with the rest of the frame the page's dark background, Session stopped after its header, and
the zoom re-snapshots at 100 % and 200 % showed the very same half state (a "Cool down" card frozen
mid-fade). Same picture as the user's screenshot.

**Root cause** (from the run's `web` status log): Today settled 2 300 ms after load, i.e. at
`__sawitSettle`'s 2 s cap on running animations, so the snapshot caught the entrance halfway; then
the view is hidden, which pauses its CSS animations, so every later snapshot (zoom, menu) pictured
the same frozen frame. Session settled after 154 ms, before its timers revealed anything, and
screens ignored their own DOM changes after that. Covers were not the cause here (they are white),
but could outlive their build. Blur: a texture was accepted up to 1.5× upscaled (taken during a zoom
animation or at an earlier zoom), and the snapshot scale was capped at 2.

**Fix:**
- Settle waits for fonts and decoded images, gives running animations and transitions 1.2 s, then
  **finishes** every finite one (twice, two frames apart), so the picture shows the page's end state.
- `contentBottom` in the probe + a Rust half-drawn check (`web/drawn.rs`): a flat band over the lower
  part of the page's content means half-drawn; the frame keeps its previous picture (or shows
  "Loading…") and is settled and snapshotted again, up to 3 times, 0.7/1.4/2.1 s apart.
- A screen's page that changes itself within 10 s of its load (timer reveals) is pictured again.
- A settle that timed out is pictured again when the page's late answer comes.
- Covers: a newer write of a screen drops every older cover of that frame in every lane; a cover
  never lasts more than 30 s after its op.
- Sharp: retaken when drawn > 10 % larger than taken; cap raised to 3× (limited by a 24 Mpx and
  16 000 px texture budget).
- "Loading…" pill on a screen with no good picture yet.
- `canvas_screen_get` and `canvas_export` wait (12 s at most) for the frame to be at rest before
  answering its picture.

**Tests:** `web::drawn` (6 unit tests: whole, cut off, short page, sparse lower half, 2× rows,
nothing to tell), `snapshots_are_retaken_sharp_and_capped`, replay
`a_newer_write_clears_the_old_covers_and_covers_expire`; e2e `screens: three screens written in a
row are pictured whole and sharp` passes at 45 %, 100 % and 200 % (all `whole: true`, textures at
0.45/1/2 points per CSS px; screenshots checked by eye: complete and sharp). Live-cursors e2e pass.

**Not verified:** reopening the canvas mid-build by hand (covered by the cover limit and `cancel()`
on load, by reading the code).

Note: the e2e control socket once answered `""` (socket closed without a reply) under heavy machine
load, in both the pre-fix and fixed builds; the spec retries reads. Not chased.

## B15: editable screens, Present and Preview, every button checked, panels

Status: **partly done** (functional fixes and the parts that reuse B16; the big parts are not done).

Done:
- **Present works for screens:** the player draws each screen in the frame on show from its page's
  picture (taken again at 2× while Present shows) instead of the blank frame fill; Back and Close
  have icons and key hints. `app_state.canvases[].view.presenting` / `.preview` report it.
- **Timer and vote pill only when used:** hidden unless a timer runs or a vote is on or has
  results, never over Preview or Present, moved to the bottom right above the toolbar (off the
  screen-bar row). Started from View › Start Timer (5 min) / Start Vote (new Sawit commands).
- **Preview bar:** after merging `backlog`, the popup track's B16 bar (one size menu, icons) is
  kept; I added the devices grouped Mobile / Tablet / Laptop / Desktop with headings, and a Reload
  button.
- **Prototype tab:** Present full-width with a play icon and its key in the tooltip; Interactions
  and Flows as sections with icons and empty states ("No interactions. Select a layer and add
  one."), add buttons with a + icon, flow rows with icon buttons (present, remove).
- **Layers panel:** a screen row has its own icon and an "HTML" tag (tooltip explains its content
  is a web page); route frames a globe; a sticky, shape or label still called by its kind shows
  its first words ("Ship the onboarding flow by Frid…"), so two stickies no longer look alike.
- **Control audit:** `docs/notes/2026-11-07-canvas-controls.md`.
- Design sketch on the canvas: `docs/design/b15.sawit` (Preview bar, size menu, Present bar).

Not done (decided, to report rather than half-build):
- Layers as the source of truth for UI, **Convert to Layers**, in-place HTML editing, and changing
  `canvas_begin`'s UI default from `kind: "screen"`: not started. (The new starter system, token
  binding and lint from B12 make the layers path workable for agents.)
- Right panel fitted to the selection (a screen shows only name, size, position, Preview, Export),
  compact two-column fields, collapsible sections; left panel compact pages / search in header /
  28 px rows: not done.
- Preview zoom fit/100 %, device frame, Open in Browser for screens, Custom size: missing.
- Per-control e2e for every family: only Preview/Present/timer/layers/prototype covered.

## B4: every canvas function through MCP

Status: **done** (coverage table and test).
- `sawit_board::mcp_paths`: each id a person reaches (main menu, context menu for an empty canvas
  and a selection, toolbar tools of both kinds of file, flyouts, every default key, Sawit's own
  commands, the UI commands, the clipboard, Send to Agent) maps to `canvas_command` (engine
  commands an agent may run), another command without its dialog (`file.placeImage` →
  `image.place {path}`), a dedicated tool (`canvas_screen_export`, `canvas_handoff`,
  `canvas_settings`, `canvas_add_routes`, `canvas_export`, `app_maximize`…), automatic (save) or
  user-only with the reason (the view, `page.select`, Follow agent, Present).
- `APP_CONTROLS`: the app's own controls (Preview, Present, Live, Send to Agent, panels, timer…)
  with their paths; `canvas_commands` now answers them as `app_controls`.
- Tests: `every_canvas_function_has_an_mcp_path` (it found Copy as Text/CSS/SVG/PNG unmapped,
  now mapped), `every_engine_command_is_reachable_or_the_users`, and in sawit-mcp
  `every_path_names_a_tool_that_exists`.
- Decision: agents still don't move the view or the page on show (existing rule); those are
  "user only" in the table, not new tools.

## B4.1: zoom to fit while an agent draws, Follow agent

Status: **done**.
- First op of an agent session: the view glides (420 ms, ease-out, log-space zoom) to fit the
  page; under Reduce Motion it jumps.
- **Follow agent** toggle on the toolbar (Locate / LocateFixed icon), shown while an agent worked in
  the last 20 s, on by default and with each new session. Changes within 150 ms go in one move; the
  view only pans while the shape fits, zooms out only when it would be off screen, never in. Any
  pan or zoom by hand (detected as the view differing from where follow left it) turns it off.
- `app_state.canvases[].view.follow {on, agent_active}`: agents read it, only the user sets it.
- Tests: `follow::tests` (5: on screen stays, off screen pans, too big zooms out never in, glide
  lands, hand move stops); e2e live-cursors checks follow is reported on.
- Not verified: a 30-shape diagram followed start to finish by eye (the e2e builds 4×5 elements;
  the screenshots show the fitted view).

## B4.2: agent logo on its cursor, random names

Status: **done for the canvas**; browser cursors not done.
- `Roster`: a second session of a client without a lane name, and the anonymous agent, get a
  random two-word name (16 adjectives × 16 fruits/plants, all under 16 characters), seeded by FNV
  of the agent's key so it is stable, moved on while a live agent has it. Names show wherever the
  label does (cursor pill, roster, activity text, app_state).
- The canvas cursor pill has the client's logo on a 14 px white disc (colour logos rasterised at
  device size; one-colour logos and the generic agent mark in near-black), legible on light and
  dark pages and every agent colour. `client_logo()` maps client names to `assets/agent_logos`.
- Tests updated (`Claude Code 2` → random name; pill geometry) plus
  `random_names_are_short_stable_and_distinct`.
- Not done: the logo on the **browser's** agent cursors (drawn by the page overlay JS, which the
  pagejs track is moving to Rust); the screenshot from the e2e run shows "Bright Kiwi" with its pill.

## B12: harden the canvas design MCP

Status: **mostly done**.
- `sawit_board::checks::lint` in Rust, one rule table (`lint/rules.rs`) with each rule's guide
  section and the standard's own ids: col.10, a11y.2, typ.11, spc.1, lay.18 (existing machine
  checks), cpy.19 placeholder copy, col.16 colours off the tokens (nearest token, `variable.bind`
  command), cnv.15 UI without a system, cnv.13 copies that should be a component (≥3 alike,
  `component.create` command), cnv.11 frames placed by hand (`layout.add` command), cnv.12 uneven
  gaps, typ.4 > 6 type sizes, col.4 > 10 colours, col.6 purple-to-blue gradient, cmp.1 emoji as
  icons, pol.5 glow/blur/glass everywhere, cmp.9 card in card, lay.13 everything centred.
- `canvas_lint` tool (works with the app closed), lint summary in the replies of
  `canvas_add_elements` / `canvas_update` for UI works, `canvas_begin {sketch}` silences warnings,
  `canvas_begin` answers `design_status` (no system → system → components → screens → cover) and a
  warning when a step is skipped.
- Tokens over raw values: a colour may name a token (`"backgroundColor": "accent"`), and any solid
  fill/stroke a write leaves that equals a token is bound to it.
- `canvas_starter_system`: Sawit's tokens as variables (Light/Dark colours, spacing, radii, type)
  and Button / Primary, Button / Secondary, Input, Card components bound to them with auto layout;
  the lint finds no raw colours in it.
- `canvas_design_guide` ends with the lint's rule table; `section: "lint"` gives it alone.
- Tests: lint on a clean vs sloppy page, nearest token, workflow stage, token binding, starter.
- D1 hook: device sizes still come from `frame.create {preset}` / `DEVICES`; Preview's size groups
  are computed by width, so D1's new sizes fall into place.
- Not done: the "done when" end-to-end run (an agent building a SaaS dashboard through MCP only,
  with before/after lint numbers); component detection when an agent adds a group shaped like an
  existing component (only repeated copies are flagged); `canvas_screen_get` / `canvas_export`
  returning lint findings (they return pictures; `canvas_lint` is separate).

## Commits (branch `wt/canvas`, in order)

1. `a869ac5e` fix(canvas): screens are pictured whole and sharp, never half-drawn or black (B17)
2. `9800db54` Merge backlog: B16 and B13 for B15's panels (as the coordinator asked)
3. `4572f42a` feat(canvas): Present shows screens, the timer pill only when used, and clearer
   panels (B15; also carries a B17 follow-up: a half-drawn screen's view keeps running until its
   retake)
4. `74a2978d` feat(canvas): every canvas function has an MCP path, kept by a test (B4)
5. `12c66367` feat(canvas): fit the view when an agent starts, and Follow agent (B4.1)
6. `97f44c8a` feat(canvas): agents' cursors show their client's logo, and a second agent gets a
   random name (B4.2)
7. `9d3913aa` feat(canvas): a design lint, tokens over raw values and a starter system (B12;
   README carries the MCP table lines of B4/B4.1/B4.2/B12 and the tool count 171)
8. `b0e07b32` fix(canvas): app_state reports every canvas tab's view, and Present is checked end
   to end (B15, B4.1)

Final e2e on the last build: `screens` (B17 three screens at 45/100/200 %, Present of a screen in
a design file) and `live-cursors` (incl. follow state): 42 pass, 0 fail. Present screenshot shows
the screen's page sharp on the stage; the Layers panel shows the screen row with its HTML tag; the
toolbar shows Follow agent after the view fitted the agents' work.

Intermediate commits were staged file by file (and with filtered versions of shared files); only
the final tree was built and tested. Tests on the final tree: sawit-board, sawit-mcp, sawit-core
agents and line limit, sawit app (905) all pass; clippy clean for the touched crates.

Notes for the coordinator:
- `crates/app/src/assets.rs` is 504 lines (backlog already had 502 after the merges); the line-limit
  test also fails on other tracks' files (accounts, browser, seo) in `backlog`.
- `cargo fmt --all` reformats `crates/core/src/site_design/page.rs` (another track's); I left it.
- e2e `covers`, `find`, `focus` failed once in my run while I was also driving the user's Sawit
  (focus stolen); not re-run.

## Design review (better-interface) of the rendered B15/B4.1/B4.2 UI

Scope: what this track rendered in its own build (e2e screenshots, dark theme only, default
window): the screen row in the Layers panel (`panels.png`), Present of a screen (`present.png`),
the Follow agent toolbar button, the agent cursor pill with logo, the timer pill's placement.
Preview's bar is the popup track's (B16) and was not re-reviewed. Light theme, narrow window and
the Prototype tab could not be captured (agent-desktop's accessibility snapshot timed out on the
canvas with live web views, so the tab couldn't be clicked).

Domains: accessibility, writing, UI loaded and applied; layout, typography, colours **not
reviewed** (skills not loaded in this run).

| Severity | Location | Finding | Status |
|---|---|---|---|
| MEDIUM | `layers_panel/rows.rs` (HTML tag) | The tag is text-only and mouse-tooltip only; its meaning is in the tooltip | Kept: the row's name and icon carry the role; tooltip is extra. Not verified with VoiceOver |
| MEDIUM | `design_panel/*` (right panel on a screen) | A screen still shows Fill, Auto layout, Resizing, which don't change it | Not fixed (B15 remaining work) |
| LOW | `player.rs` bar | Hard-coded dark stage colours (`0x1e1e1e`, white text) rather than theme tokens | Pre-existing; not changed |
| — | `toolbar/mod.rs` Follow agent | Icon-only toggle has an accessible name ("Follow agent"), selected state and tooltip | OK |
| — | `presence/paint.rs` pill | Logo on a white disc keeps contrast on every agent colour | OK by eye in the e2e screenshot |

Verdict: **Approve with the open MEDIUM above** (no HIGH found in what was inspected; light theme
and narrow window not verified).
