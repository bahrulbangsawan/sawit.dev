# Browser Preview 2: Annotate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Annotate panel. The user points at elements in the Browser tab, comments, and sends
structured markdown (selectors, styles, source file, screenshot path, console errors) to Claude.
There are three ways out: Copy, Send to terminal, and the `browser_annotations` MCP tool.

**Architecture:**
- **Core (`sawit_core::annotations`):** pure; owns the model, numbering, file names and the
  markdown.
- **Page side:** the overlay's `picked` event carries the full element details, so a pick is one
  round trip.
- **App:** `AnnotatePanel` (GPUI) holds the rows. `BrowserView` captures the screenshot, keeps
  markers in sync, and emits `BrowserEvent::SendToTerminal`, which the workspace pastes only into
  a terminal that accepts bracketed paste.

**Tech Stack:** Rust, GPUI (gpui-kit 0.8.1: `Input`, `Checkbox`, `Button::selected`), libc (`localtime_r`), plain JS.

**Spec:** `docs/superpowers/specs/2026-10-29-browser-annotate-design-design.md` (sections "Annotate panel" and "Annotation markdown")

## Global Constraints

- Plan 1 is merged. It provides `page_op`, `shoot`, the `markers`/`flash` ops, `Panel`,
  `PickMode`, `ops_after_load` and `panel::frame`.
- **Intents:** `fix` (default), `change`, `question`, `copy`.
- **Markdown cuts:** text at 100 characters with `…`; HTML at 20 000 characters with
  `<!-- cut -->`, and only for the `copy` intent.
- **Styles:** `display` always, then the non-default values of position (≠ static), overflow
  (≠ visible), z-index (≠ auto), opacity (≠ 1), background-color (≠ transparent / `rgba(0, 0, 0,
  0)`), color (≠ `rgb(0, 0, 0)`), font-size (≠ 16px), font-weight (≠ 400), border-radius (≠
  0px), gap (≠ normal), padding (≠ 0px), margin (≠ 0px). In that order, joined by `; `.
- **Screenshots:**
  - Taken at pick time, with the element rect padded 8 px and clamped to the viewport.
  - Saved to `.nyawit/annotations/<yyyy-mm-dd>/<host>[-<port>]-<n>.png` via `nyawit::ensure` +
    `nyawit::unique`.
  - Without a project they go to `AppPaths::default_macos().cache_dir()/annotations/<date>/`.
- **Numbers** count per page URL (URL without the hash).
- **Console errors:** current page only, after its last `— navigated to` marker, newest 20, shown
  as `+12.3s` since page start.
- **Send to terminal:**
  - It pastes only into a tab whose program enabled bracketed paste: the active tab if it
    qualifies, else the highest-numbered tab that does. It never presses Enter.
  - Otherwise it copies and notes "No terminal is ready for a paste, so it was copied."
- **Footer notes** show for 3 s: "Copied", "Sent to Terminal N" (N = tab index + 1).

## Review Focus

1. **Send to terminal when the active tab is a dev server or a plain prompt without bracketed
   paste.** Nothing is pasted; the text is copied. Covered by Task 4 `paste_target` tests.
2. **Two annotations on different URLs of one host with the same number.** Two screenshot files,
   not one overwritten. Covered by Task 3 test `screenshot_names_never_collide`, using
   `nyawit::unique`.
3. **An SPA route change (pushState) between picks.** Rows stay, grouped under their URLs, and
   markers show only for the current URL. Covered by Task 3 `group_order` test and Task 6 step.
4. **A comment containing markdown or backticks.** It is copied verbatim and doesn't break the
   structure, because the comment sits on its own `- **Comment:**` line. Covered by the Task 1
   test `comment_is_verbatim`.
5. **A pick on an element partly off-screen.** The screenshot is clamped and the path is followed
   by "(partly off-screen)". Covered by the Task 1 markdown test and the Task 3 `padded_rect`
   test.

---

### Task 1: The annotation model, markdown and dates (core)

**Files:**
- Create: `crates/core/src/annotations.rs`, `crates/core/src/date.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod annotations; pub mod date;`), `crates/core/src/browser.rs` (`Logs::page_errors`)

**Interfaces — Produces:**
- **`date.rs`:**
  - `pub fn civil_from_days(days: i64) -> (i64, u32, u32)`: Hinnant's inverse of
    `github.rs`'s days-from-civil.
  - `pub fn local_date() -> String`: `yyyy-mm-dd` in local time, using `libc::localtime_r`'s
    `tm_gmtoff`.
- **`annotations.rs`:**
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "lowercase")] pub enum Intent { #[default] Fix, Change, Question, Copy }`,
    with `pub const ALL: [Intent; 4]` and `pub fn label(self) -> &'static str`.
  - `pub struct Rect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }` (serde).
  - `pub struct SourceHint { pub file: String, pub line: Option<u32>, pub framework: String, pub component: Option<String> }` (serde).
  - `pub struct ElementDetails { pub selector: String, pub label: String, pub tag: String, pub id: Option<String>, pub classes: Vec<String>, pub text: Option<String>, pub rect: Rect, pub styles: Vec<(String, String)>, pub source: Option<SourceHint>, pub html: Option<String>, pub off_screen: bool }`.
    Serde, every field but `selector`/`label`/`tag`/`rect` `#[serde(default)]`.
  - `pub struct Annotation { pub n: u32, pub url: String, pub element: ElementDetails, pub comment: String, pub intent: Intent, pub screenshot: Option<PathBuf>, pub include_screenshot: bool }`
  - `pub struct PageInfo { pub title: String, pub url: String, pub viewport: (u32, u32), pub device: Option<String> }`
  - `pub struct PageGroup { pub page: PageInfo, pub items: Vec<Annotation> }`
  - `pub fn markdown(groups: &[PageGroup], errors: Option<(&str, &[ConsoleEntry])>) -> String`
  - `pub fn next_number(existing: &[Annotation], url: &str) -> u32`
  - `pub fn page_key(url: &str) -> &str`: the URL without `#…`.
  - `pub fn screenshot_dir(project: Option<&Path>, cache: &Path, date: &str) -> PathBuf`
  - `pub fn screenshot_name(url: &str, n: u32) -> String`
  - `pub fn relative_source(file: &str, project: Option<&Path>) -> String`
- **`browser.rs`:** `impl Logs { pub fn page_errors(&self, limit: usize) -> Vec<ConsoleEntry> }`.

- [ ] **Step 1: Write failing tests.**
  - `markdown_matches_the_spec`: one group, built from the spec's example values
    (`button.btn-primary`, fix, comment, selector `main > div > button.btn-primary`, text
    "Submit order", source `src/components/Checkout.tsx` line 42 React `Checkout`, rect
    350,680,120,40, three styles, screenshot path, viewport 393×852 device "iPhone 15"), plus
    `errors = Some(("http://localhost:5173/cart", &[one error at 12_300 ms from app.js line 88]))`.
    It equals this literal exactly:
    ```text
    # Visual feedback — Cart
    **URL:** http://localhost:5173/cart · **Viewport:** 393×852 (iPhone 15)

    ## 1. `button.btn-primary` — fix
    - **Comment:** The button should use the brand blue, not green.
    - **Selector:** `main > div > button.btn-primary`
    - **Element:** `<button class="btn-primary">` "Submit order"
    - **Source:** `src/components/Checkout.tsx:42` (React, <Checkout>)
    - **Box:** 120×40 at (350, 680)
    - **Styles:** display: flex; background-color: rgb(34, 197, 94); font-size: 14px
    - **Screenshot:** /p/.nyawit/annotations/2026-10-29/localhost-5173-1.png

    ## Console errors on http://localhost:5173/cart (1)
    - `+12.3s` Uncaught TypeError: x is undefined — app.js:88
    ```
  - `empty_fields_are_left_out`: no comment, no text, no source, no screenshot. Those four
    lines are absent.
  - `copy_intent_adds_html`: intent `Copy` with `html` of 25 000 `a`s gives a fenced `html`
    block that is 20 000 characters long and ends with `<!-- cut -->`.
  - `comment_is_verbatim`: a comment with backticks and `## heading` appears unchanged after
    `- **Comment:** `.
  - `off_screen_is_noted`: the screenshot line ends with ` (partly off-screen)`.
  - `groups_keep_their_order`: two groups → two `# Visual feedback` headers, in the given order.
  - `numbers_count_per_url`: `next_number` is 3 for a URL with 1 and 2, and 1 for a new URL.
    `#hash` URLs count with their page.
  - `screenshot_names`: `screenshot_name("http://localhost:5173/cart?x=1", 1) == "localhost-5173-1.png"`
    and `("https://www.apple.com/iphone/", 2) == "www.apple.com-2.png"`.
  - `relative_sources`:
    - `("webpack:///./src/App.tsx", None) == "src/App.tsx"`
    - `("/Users/me/app/src/App.tsx", Some(Path::new("/Users/me/app"))) == "src/App.tsx"`
    - `("/@fs/Users/me/app/src/x.vue", Some(..)) == "src/x.vue"`
    - `("file:///opt/x.js", None) == "/opt/x.js"`
  - `page_errors_since_navigation`: an error, then the marker `— navigated to …`, then 25
    errors → the newest 20.
  - `civil_dates`: `civil_from_days(20390) == (2025, 10, 29)` and
    `civil_from_days(0) == (1970, 1, 1)`.
- [ ] **Step 2:** Run `cargo test -p sawit-core annotations date browser`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Markdown line order: Comment, Selector, Element, Source, Box, Styles, Screenshot, then the
    html block.
  - The Element line: `<tag id="…" class="…">` plus the quoted text.
  - The Source line: `file[:line]` plus `(Framework[, <Component>])`.
  - The Box line rounds to integers.
  - Error lines: `` - `+{s:.1}s` {text} — {file name of url}:{line} ``. The ` — …` tail is left
    out when there is no URL.
- [ ] **Step 4:** Run `cargo test -p sawit-core`. Expected: PASS.
- [ ] **Step 5:** Commit `feat(annotate): annotation model, markdown and screenshot names`.

### Task 2: Element details in the `picked` event (page)

**Files:**
- Modify: `crates/core/src/browser_overlay.js`, `crates/core/src/annotations.rs` (contract test)

**Interfaces — Produces:** `picked.element` now has every `ElementDetails` field.
- `text`: `innerText`, trimmed and collapsed.
- `styles`: an array of `[property, value]` pairs per Global Constraints.
- `html`: `outerHTML`, cut at 20 000 characters.
- `off_screen`: the rect isn't fully inside the viewport.
- `source`: per the algorithm in Step 3.

- [ ] **Step 1: Write a failing contract test.** `picked_payload_deserializes` parses this JSON,
  shaped exactly as the overlay will send it, into `ElementDetails`:
  `{"selector":"#a","label":"button#a","tag":"button","id":"a","classes":[],"text":"Go","rect":{"x":1,"y":2,"width":3,"height":4},"styles":[["display","flex"]],"source":{"file":"src/A.tsx","line":7,"framework":"React","component":"A"},"html":"<button id=\"a\">Go</button>","off_screen":false}`.
  Assert `styles[0] == ("display","flex")` and `source.line == Some(7)`.
- [ ] **Step 2:** Run `cargo test -p sawit-core annotations`. Expected: FAIL until the
  `Deserialize` derives match. Fix the derives, not the JSON.
- [ ] **Step 3: Implement `details(el)` in the overlay.** The source-hint algorithm, first match
  wins:
  ```text
  React:  key on el starting "__reactFiber$" → fiber; walk fiber, fiber.return… (max 50):
            if f._debugSource → {file:f._debugSource.fileName, line:f._debugSource.lineNumber}
            first f.type that is a function/class → component = type.displayName || type.name
          → {framework:"React", file, line, component}  (file absent → use only component;
             no component and no file → no hint)
  Vue 3:  el.__vueParentComponent?.type → {framework:"Vue", file:type.__file, component:type.name||type.__name}
  Vue 2:  el.__vue__?.$options → {framework:"Vue", file:__file, component:name}
  Svelte: el.__svelte_meta?.loc → {framework:"Svelte", file:loc.file, line:loc.line+1}
  Attrs:  closest [data-source] "file:line" → {framework:"source"}; or closest
          [data-inspector-relative-path] + data-inspector-line
  ```
  Climb `parentElement` until a hint is found. The app later passes `file` through
  `relative_source`. With no file at all, the Source line is left out (Task 1).
- [ ] **Step 4:** Run `node --check crates/core/src/browser_overlay.js && cargo test -p sawit-core annotations`. Expected: pass.
- [ ] **Step 5:** Commit `feat(annotate): element details, key styles and source hints on pick`.

### Task 3: AnnotatePanel, picking flow, screenshots and markers (app)

**Files:**
- Create: `crates/app/src/browser/annotate.rs`
- Modify: `crates/app/src/browser/mod.rs`

**Interfaces — Consumes:** Task 1 types, `page_op`, `shoot` and `ops_after_load` (Plan 1).
**Produces:**
- `pub struct AnnotatePanel`, with:
  - `new(window, cx) -> Self`
  - `pub fn add(&mut self, a: Annotation, window, cx)`: appends and focuses the row's comment
    `Input`.
  - `pub fn annotations(&self, cx: &App) -> Vec<Annotation>`: comments are read from the inputs.
  - `pub fn markers_for(&self, url: &str) -> Vec<(u32, String)>`
  - `pub fn clear(&mut self, cx)`
  - `pub fn set_note(&mut self, text: impl Into<SharedString>, cx)`
  - `pub fn include_errors(&self) -> bool`
- `pub enum AnnotateEvent { Show(String), Changed, Copy, Send }`, with
  `impl EventEmitter<AnnotateEvent> for AnnotatePanel {}`.
- `fn padded_rect(r: &Rect, viewport: (u32, u32), pad: f64) -> (f64, f64, f64, f64)`
- `fn group_order(current: &str, items: &[Annotation]) -> Vec<String>`: page keys, the current
  page first, then first-seen order.
- **`BrowserView`:**
  - `fn on_picked(&mut self, element: ElementDetails, window, cx)`
  - `fn sync_markers(&self)`
  - `pub fn annotations_markdown(&self, cx: &App) -> Option<String>`: `None` when there are no
    annotations.

- [ ] **Step 1: Write failing tests.**
  - `padded_rect(&Rect{x:5.,y:5.,width:100.,height:40.}, (393,852), 8.) == (0.,0.,113.,53.)`.
  - `padded_rect` of a rect at x 380 width 50 → width clamps to `393 - 372 = 21`.
  - `group_order("http://a/2", [a/1 n1, a/2 n1, a/1 n2]) == ["http://a/2", "http://a/1"]`.
  - `screenshot_names_never_collide` (in `annotations.rs`, using `nyawit::unique` in a
    tempdir): two `localhost-5173-1.png` saves → `localhost-5173-1.png` and
    `localhost-5173-1-2.png`.
- [ ] **Step 2:** Run `cargo test -p sawit browser && cargo test -p sawit-core annotations`. Expected: FAIL.
- [ ] **Step 3: Panel UI** (inside `panel::frame("Annotate", …)`).
  - Groups have a URL header (path only; host in the tooltip). The current page is first.
  - Each row:
    - `n` badge and the label in monospace.
    - Comment `Input` (multi-line, placeholder "What should change?").
    - Four xsmall `Button`s for the intents with `.selected(intent == i)`.
    - Lucide `Camera` toggle (`.selected(include_screenshot)`).
    - Lucide `Eye` "Show" → `AnnotateEvent::Show(selector)`.
    - Lucide `Trash2` "Delete" (use `Trash` if `Trash2` isn't in the icon set).
  - Empty state: "Click an element on the page to annotate it. Esc stops picking."
  - Footer: `Checkbox` "Include console errors" (on), `Copy`, `Send to terminal`, ghost
    `Clear`, and the note line.
- [ ] **Step 4: Flow in `BrowserView`.**
  1. `Picked` with mode `annotate` → deserialize `ElementDetails`.
  2. Set `source.file = relative_source(file, project)`.
  3. `n = next_number(..)`.
  4. `shoot(Some(padded_rect))`, then write the PNG to
     `nyawit::unique(screenshot_dir(..), screenshot_name(..))`. A failed capture keeps the
     annotation with `screenshot: None`.
  5. `panel.add(..)`, then `sync_markers()`.
  - `sync_markers` sends `{"op":"markers","items":[…]}` for `page_key(current_url)`. It is
    added to `ops_after_load` and runs after add, delete and clear. It also runs on
    `PageEvent::Url`, because SPA route changes fire no load event.
  - `Show(selector)` → `{"op":"flash","selector":…}`.
  - `annotations_markdown`: `PageInfo` from the title, the URL, `describe()["viewport"]` and
    the device name. Errors come from `logs.page_errors(20)` when `include_errors`.
  - `Copy` → `cx.write_to_clipboard(ClipboardItem::new_string(md))` + note "Copied".
- [ ] **Step 5:** Run `cargo test -p sawit browser && cargo build -p sawit`. Expected: pass.
- [ ] **Step 6:** Commit `feat(annotate): annotate panel with screenshots and markers`.

### Task 4: Send to terminal

**Files:**
- Modify: `crates/app/src/terminal/view.rs`, `crates/app/src/terminal/panel.rs`, `crates/app/src/browser/mod.rs`, `crates/app/src/workspace.rs`

**Interfaces — Produces:**
- `TerminalView`:
  - `pub(crate) fn paste(&mut self, text: &str)`: sets `touched` and writes
    `paste_bytes(text, self.modes())`. `on_key`'s ⌘V uses it too.
  - `pub(crate) fn accepts_paste(&self) -> bool`: `modes().bracketed_paste && !exited`.
- `TerminalPanel`:
  - `pub fn paste_target(&self, cx: &App) -> Option<usize>`
  - `pub fn paste(&mut self, ix: usize, text: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String>`:
    activates `ix`, pastes, then `focus_active`.
- `fn pick_paste_target(active: usize, accepts: &[bool]) -> Option<usize>` (pure).
- `pub enum BrowserEvent { SendToTerminal(String) }`, with
  `impl EventEmitter<BrowserEvent> for BrowserView {}`.
- `BrowserView::set_annotate_note(&mut self, text: &str, cx)`

- [ ] **Step 1: Write failing tests** (`terminal/panel.rs`):
  - `pick_paste_target(1, &[true,false,true]) == Some(2)`
  - `(0, &[true,false]) == Some(0)`
  - `(0, &[false,false]) == None`
  - `(0, &[]) == None`
- [ ] **Step 2:** Run `cargo test -p sawit terminal`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - `AnnotateEvent::Send` → `BrowserView` emits `SendToTerminal(markdown)`.
  - The workspace subscribes where it creates the browser view in `open_browser`.
    - Some target: set `layout.terminal = true`, paste, then note "Sent to Terminal {ix+1}".
    - `None`: write to the clipboard, then note "No terminal is ready for a paste, so it was
      copied."
- [ ] **Step 4:** Run `cargo test -p sawit terminal browser && cargo build -p sawit`. Expected: pass.
- [ ] **Step 5:** Commit `feat(annotate): send annotations to the terminal running Claude`.

### Task 5: `browser_annotations`

**Files:**
- Modify: `crates/core/src/browser.rs`, `crates/app/src/browser/mod.rs`, `crates/mcp/src/browser.rs`, `crates/mcp/src/lib.rs`

**Interfaces — Produces:** `BrowserOp::Annotations { #[serde(default)] clear: bool }` (native).
It answers `{"markdown": String|null, "count": n}`. With `clear`, the rows and markers are
removed after the answer is built.

- [ ] **Step 1: Write failing tests.**
  - `BrowserOp` round-trips `{"op":"annotations","clear":true}`.
  - `is_native()` is true for it.
  - The `lib.rs` tool-name list includes `"browser_annotations"`.
- [ ] **Step 2:** Run `cargo test -p sawit-core browser && cargo test -p sawit-mcp`. Expected: FAIL.
- [ ] **Step 3: The tool.**
  - Description: "Read the annotations the user made in the browser tab: what to change, on
    which element (selector, styles, source file), with screenshot paths. clear removes them
    after reading".
  - Args: `clear`, `project`.
  - Returns the markdown as text. With none it returns "No annotations yet. The user adds them
    with Annotate (⌘⇧.) in the Browser tab."
  - The INSTRUCTIONS `browser_*` line gains "read the user's annotations".
- [ ] **Step 4:** Run `cargo test -p sawit-core && cargo test -p sawit-mcp && cargo build -p sawit -p sawit-mcp`. Expected: pass.
- [ ] **Step 5:** Commit `feat(mcp): browser_annotations reads the user's visual feedback`.

### Task 6: End-to-end check

**Files:**
- Create: `docs/fixtures/browser/annotate.html`. It contains:
  - `<main><div><button class="btn-primary" data-source="src/components/Checkout.tsx:42">Submit order</button></div></main>`;
  - a link `#/two` that does `history.pushState`;
  - a script that logs `console.error("Uncaught TypeError: x is undefined")` on load.

- [ ] **Step 1:** Launch the dev app (memory recipe) with this repo as the project, and serve
  the fixtures (Plan 1 Task 6).
- [ ] **Step 2: Through `sawit-mcp`:**
  1. Navigate, press `Meta+Shift+.`, then click `.btn-primary`.
  2. `browser_annotations` contains:
     - `## 1. \`button.btn-primary\` — fix`
     - `**Source:** \`src/components/Checkout.tsx:42\``
     - a Screenshot path under `.nyawit/annotations/` that exists on disk (`ls`)
     - `## Console errors on`
  3. Click the `#/two` link (pick mode off first: Esc via `browser_press_key "Escape"`).
     Press ⌘⇧., then click `.btn-primary` again. `browser_annotations` has two `# Visual
     feedback` headers, and the second annotation is numbered 1.
  4. `browser_annotations clear=true`, then again → "No annotations yet…".
- [ ] **Step 3:** Take a window screenshot of the panel with two rows. Check the badges, intent
  buttons, footer and note.
- [ ] **Step 4:** Commit the fixture and fixes: `test(annotate): fixture and fixes`.
