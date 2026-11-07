# pagejs track report (Part A: page JavaScript logic to Rust)

Worktree `/Volumes/bahrul/project/sawit-wt/pagejs`, branch `wt/pagejs`.

## Method for "no behaviour change"

- Baseline binary built from an export of `e04558a` (`target-pagejs/sawit-baseline`).
- `e2e/specs/page-golden.test.js` reads the fixture pages in `docs/fixtures/browser/` (with a new
  `golden.html` that holds oklch/lab/display-p3 colours, translucent layers, focus rings, every
  form control state, ARIA roles, a shadow root and an open dialog) and compares `browser_snapshot`,
  every part of `browser_design` and `browser_inspect` against `e2e/fixtures/golden/*.json`,
  captured with the baseline binary. With `SAWIT_GOLDEN_URLS`/`SAWIT_GOLDEN_OUT` it also dumps
  the same answers for real sites, compared baseline vs new by hand.

## Items

### A1 colour math to Rust: `2bdcee9`

- New `site_design::computed` (exact port of `color_math.js`, incl. edge cases like `grad` read
  as radians and NaN parts) and `site_design::page` (page wire types + the decisions:
  transparent colours dropped, colours ranked/capped at 500, body background, focus ring,
  primary/secondary button pick, 2 MB budget fitting with JS-exact JSON sizes, effective
  background compositing). The scan is now two page calls: `scan` (collect) and
  `scan_samples` (sample the components Rust picked).
- Canvas fallback for unknown colour syntax stays JS: the page sends the browser's reading for
  colours outside the syntaxes Rust reads (`color_fallbacks`).
- Tests: `site_design::computed` tests compare Rust against the old JS (kept as
  `site_design/tests/color_math.js`) in QuickJS over 460 colours; `site_design::page` tests;
  core `site_design` + `browser::tests` 175 pass; app `browser::design` 44 pass; clippy clean.
- Golden: `page golden` e2e passes against the baseline goldens (6 fixture pages: snapshot,
  6 design parts, 10 inspects each). One run had a single empty control-socket reply
  (`design.html design.fonts`), the rerun passed: flake in the socket, not the answer.
- Real sites (example.com, apple.com, tailwindcss.com, news.ycombinator.com): baseline vs A1
  differ in exactly the keys and amounts baseline vs baseline (two runs) differ (rotating
  content); summaries and tokens identical.

### A2 mesurer glue duplicates: `43f84b4`

- `capture.codec` bridge kind (Rust `files::codec_for_page`) replaces the JS AVC level table;
  `file.save` with `href` for data: links (Rust `files::data_url`) replaces `dataBlob`.
- Decision: a data: download now posts without waiting, so two simultaneous downloads may
  arrive in either order (test made order-free). A malformed data: URL now also shows
  "Couldn't save …" in the Annotate footer (before: only a log line).
- Tests: mesurer core tests 49 pass (QuickJS glue tests updated), clippy clean, e2e
  `mesurer|annotate` pass (non-headed).
- Not verified: an actual recording/screenshot saved through mesurer's buttons (headed spec
  only; it moves the real pointer, not run).

- Tests: app `browser::` 145 pass, core lib 931 pass, clippy clean; e2e `pointer` (new:
  trusted click, double click, far-down click, right click stays synthetic, the user's pointer
  unmoved, background tab synthetic), `page golden`, `tabs`, `browser`, `focus` (keyboard stays
  with the user's terminal while an agent works) all pass (46 pass, 0 fail). The
  `live-cursors` spec file matched the filter but none of its tests did; not re-run by name.

### A6: blocked (ground rule 5), not touched.

### A7 (native overlay drawing): not shipped, analysis

The backlog pictures "a GPUI layer over the web view". That layer can't exist: the `WKWebView` is
an AppKit view above GPUI's Metal view, so GPUI draws *under* a live page (`crates/app/src/browser/cover.rs`
says so, and hides the page behind a picture whenever a GPUI menu opens over it). The options
and why each loses behaviour today:

- **GPUI drawing**: impossible over a live page (above). Freezing the page into a picture while
  the overlay shows (as `cover.rs` does for menus) would stop the page while an agent drives it.
- **A Core Animation layer view above the web view** (objc2 `CALayer`/`CATextLayer`, hit-testing
  off): drawable, but (1) `browser_screenshot` takes `WKWebView.takeSnapshot`, which wouldn't show
  it, and agents' screenshots may carry the agents' cursors (`overlay {hidden, agents}` op), so
  the capture would need compositing in Rust; (2) mesurer's in-page UI layers against the overlay's
  z-index (`mesurer/host.js`): a native layer always sits above the whole page, so mesurer's
  toolbar and panels would be under Sawit's cursors and popover; (3) the device preview scales the
  page (`ui_scale`, `_setViewScale`), which the native layer would have to follow per frame;
  (4) the popover is a text editor (the note's comment field) with its own keyboard and IME
  handling inside the page, with the user's typing kept in the page; native, it is a new GPUI/AppKit
  text field floating over another process's view.
- The agents' cursors glide with CSS transitions timed against the action; a native layer would
  need its own animation clock in step with page events.

No stage could be made to keep all of: screenshots with cursors, mesurer layering, device scale,
and the popover's editing. Per the brief ("ship the stages that are solid and document the rest"),
nothing of A7 is shipped. A follow-up that wants it should first decide (with the user) whether
agents' screenshots may lose the cursors, and start with the cursors as a `CALayer` view above
the web view, composited into `engine/capture.rs` captures.

### A3 design grouping, ranking and limits: `86167fc4`

- Rust (`site_design::page`): text styles read from computed CSS text (parseFloat/Math.round
  semantics, checked against QuickJS), grouped and ranked (400 kept); spacing grouped to
  0.5 px; radii/shadows ranked; card and link picks; variables capped at 500 and breakpoints
  sorted (page sheets and fetched sheets); `Limits` sent with every design op (limits.js has no
  numbers now).
- Stays JS (reads the DOM): walk, computed styles, stylesheets, fonts, HTML, URL helpers
  (`absUrl`, `fileName`, `isSvgUrl`, used for asset labels/keys during the walk), `num`,
  `round3` (still used by `style`'s and the samples' own numbers).
- Tests: core `site_design` + design 82 pass; app `browser::design` pass; clippy clean;
  page-golden e2e 36/36 on the A3 build.
- Found: the control socket sometimes closes with an empty reply under load, in the baseline
  build too (reproduced on the baseline). The golden spec asks again on that one error.

### A4 snapshot outline in Rust: `042f06ef`

- Page sends nodes (`d`, `role`, `name`, `level`, `value`, `states`, `url`, `ref`, `holds`);
  `browser::snapshot::outline` writes the text, `snapshot::answer` replaces `nodes` with
  `snapshot` in the op answer (`crates/app/src/browser/ops.rs`).
- Decision: clipping and JSON quoting of names/values/text stay in the page. Clipping counts
  UTF-16 units and can split a surrogate pair; only the page's own `JSON.stringify` keeps that
  half pair as `\udXXX`, and serde_json rejects a lone surrogate in the outer answer. Refs stay
  page-side (the page acts on them).
- Snapshot diff (mentioned as possible in the backlog) not added: no tool asks for it yet;
  `Node` makes it straightforward (follow-up: a `browser_snapshot {diff: true}` option).
- Tests: `browser::snapshot` unit tests; core 117 browser tests; page-golden e2e (snapshot of
  6 fixture pages incl. golden.html's roles/states/shadow root/slot/dialog) passes on a build
  with A4.

### A5 native pointer events: `b90dd002`

- Left clicks (single/double) on a tab on screen: `aim` page op → `Engine::native_click`
  (mouse-moved/down/up `NSEvent`s straight to the view's handlers, CSS px mapped through the
  view's bounds / `innerWidth`, so zoom and device scale are covered) → `aimed` polling until the
  page saw the click (1.5 s), else the page agent's synthetic click.
- Keyboard rule kept: events bypass `NSWindow.sendEvent` (no first-responder change, the app's
  mouse-down monitor doesn't see them) and the page's trusted `Pressed` report is ignored for
  3 s after an agent's click.
- Stays JS (decisions): hover (tested: a mouse-moved event handed to the view never reaches
  the page, so the fallback ran every time; the hover would only have cost 1.5 s), scroll
  (the op answers exact positions; a real wheel is async, smooth and imprecise), right click
  (a real one opens WebKit's context menu, a modal tracking loop), hidden/background tabs.
- Known differences of a native click: `clicked` names the element as it was before the click
  (the JS path named it after its handlers ran); `event.buttons` comes from WebKit's reading of
  the hardware (0), as for any click WebKit didn't see the mouse press for.


## Commits (in order, branch `wt/pagejs`)

1. `2bdcee97` refactor(browser): the design module's colour math moves to Rust (A1)
2. `43f84b44` refactor(browser): mesurer's glue asks Rust for the H.264 level and data: bytes (A2)
3. `86167fc4` refactor(browser): the design scan's grouping, ranking and limits move to Rust (A3)
4. `042f06ef` refactor(browser): Rust writes the element snapshot's outline (A4)
5. `b90dd002` feat(browser): an agent's click on a tab on screen is a real mouse click (A5)

A6 blocked; A7 not shipped (analysis above). No UI was designed or changed, so no
better-interface review applies (A7, the only visual item, shipped nothing).

## Not verified

- mesurer recording/screenshot saved through its own buttons (headed spec only).
- Native click under the device preview's view scale (mapping covers it by design; the
  pointer spec runs at the responsive size only).
- Real sites can only be compared within their own run-to-run noise (rotating content).

## Follow-ups

- `browser_snapshot {diff: true}` on top of `browser::snapshot::Node`.
- The control socket's occasional empty reply under load (pre-existing; reproduced on the
  baseline build): `crates/app/src/cli/server.rs` `answer`.
- A7 needs a user decision first (see analysis).
