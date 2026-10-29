# Browser Preview 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prepare the Browser tab for Annotate and Design. This plan adds:
- an engine seam;
- the in-page overlay (picking, highlights, markers);
- a side panel with toolbar toggles and ⌘⇧.;
- device sizes with `browser_resize`;
- open-in-default-browser, and Ports opening in the Browser tab;
- screenshots that never include the overlay.

**Architecture:** `crates/app/src/browser.rs` becomes a module folder. `engine.rs` alone holds wry
and WebKit. A bare-block JS module (`browser_overlay.js`) is spliced into the page agent's IIFE at
a placeholder, adds ops to its `OPS` table, and posts `picked` / `hovered` / `key` events over the
agent's token-signed IPC. GPUI draws a 340 px side panel next to the page. Device mode sizes the
web view frame to fit and sets page zoom so the CSS viewport equals the device.

**Tech Stack:** Rust, GPUI (gpui-kit 0.8.1), wry 0.57.1 (`WebView::zoom`), objc2-web-kit 0.3.2, rmcp, plain JS (no bundle).

**Spec:** `docs/superpowers/specs/2026-10-29-browser-annotate-design-design.md`

## Global Constraints

- **Before Task 1:**
  - The uncommitted work in `main` is committed. That includes `crates/core/src/nyawit.rs`, which
    is untracked, plus `workspace.rs`, `control.rs` and `mcp/app.rs`.
  - `plans/2026-10-29-native-canvas-8-cutover.md` is amended so it keeps `wry` for
    `crates/app/src/browser/`, and its `rg` check excludes that folder.
- **macOS only.** No new crates.
- **Engine seam:** only `crates/app/src/browser/engine.rs` may name `wry`, `objc2*` or `block2`.
- **Page modules:** each is a bare block `{ … }` spliced at `/*__SAWIT_MODULES__*/` inside the
  agent IIFE. There is one IIFE, one token and one `OPS` table. Modules use the agent's `post`
  and `modules` object.
- **Overlay host:** `<sawit-overlay data-sawit-overlay>`, closed shadow root, appended to
  `document.documentElement` (never `<body>`). It is created on first use, and only once
  `document.readyState === "complete"` plus 1 000 ms have passed. It is re-appended if
  something removed it. Styles: `position:fixed; inset:0; z-index:2147483647;
  pointer-events:none`.
- **Never in what they measure:** the agent snapshot and the picker skip `[data-sawit-overlay]`.
  Every screenshot hides the overlay first and shows it again after, even when the capture fails.
- **Side panel:** 340 px wide, on the right. Modes are Annotate and Design, one at a time.
- **Devices, exactly:**

  | Name | Width × height |
  |---|---|
  | iPhone 15 | 393 × 852 |
  | Pixel 8 | 412 × 915 |
  | iPad mini | 744 × 1133 |
  | iPad Pro 11″ | 834 × 1194 |
  | Laptop | 1280 × 800 |
  | Desktop | 1440 × 900 |

  Plus Responsive.
- **Keys:** ⌘⇧. toggles Annotate (from the page or from Sawit). Esc leaves pick mode. ↑ / ↓
  or Alt+scroll move to the parent / back to the child.
- **Copy:** follows the `sawit-mcp` voice: short sentences, curly quotes in errors.

## Review Focus

1. **A React app hydrating the whole document while pick mode is armed.** No hydration warning,
   because the host waits for `complete` + 1 s and sits outside `<body>`. Covered by Task 6 step
   with `hydrate.html`.
2. **A page with its own capture-phase `click` listener on `document`.** A pick click never
   reaches it, because the picker listens on `window` in the capture phase and calls
   `stopImmediatePropagation`. Covered by Task 6 step with `pick.html`.
3. **Device mode in a window smaller than the device.** `window.innerWidth` is still the device
   width (scale < 1). Covered by Task 3 `fit` tests and the Task 6 `innerWidth == 393` step.
4. **Navigating, or a full reload, while Annotate is open.** Pick mode re-arms on the new page.
   Covered by Task 4 `ops_after_load` test and a Task 6 step.
5. **A screenshot taken while a hover box is showing.** No overlay in the PNG, and the overlay
   is back afterwards. Covered by Task 4 `screenshot_wraps_overlay` test and a Task 6 step.

---

### Task 1: Engine seam

**Files:**
- Move: `crates/app/src/browser.rs` → `crates/app/src/browser/mod.rs` (`git mv`)
- Create: `crates/app/src/browser/engine.rs`

**Interfaces — Produces** (all in `engine.rs`):
- `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Frame { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }`.
  Logical px in the window.
- `pub struct EngineSpec { pub url: String, pub init_script: String, pub data_store: Option<[u8; 16]>, pub visible: bool, pub frame: Frame }`
- `pub enum EngineEvent { Ipc(String), Load { started: bool, url: String }, Title(String), NewWindow(String) }`
- `pub struct Engine`, with:
  - `build(spec: EngineSpec, on_event: impl Fn(EngineEvent) + Clone + 'static, window: &Window) -> Result<Engine, String>`.
    Add `Send` to the bound if a wry handler requires it.
  - `load_url(&self, &str)`, `reload(&self)`, `eval(&self, &str)`.
  - `eval_with_callback(&self, script: &str, callback: impl Fn(String) + Send + 'static) -> Result<(), String>`.
  - `url(&self) -> Option<String>`.
  - `set_frame(&self, Frame)`, `set_visible(&self, bool)`, `set_zoom(&self, scale: f64)`.
  - `focus(&self)`, `focus_parent(&self)`, `open_devtools(&self)`.
  - `snapshot(&self, css_rect: Option<(f64, f64, f64, f64)>, css_width: f64, scale: f64, reply: oneshot::Sender<Result<Value, String>>)`.

`BrowserView` keeps `engine: Option<Rc<Engine>>` (renamed from `webview`). `FromPage` gains
`Engine(EngineEvent)`. The IPC loop maps `Ipc(body)` through `parse_ipc(&body, &self.token)`.

- [ ] **Step 1: Move, then build `engine.rs`.**
  - It holds `build` (today's `BrowserView::build`), the navigation policy (`is_web` + `open` for
    other schemes), `rect`, `snapshot` and `at_1x`.
  - Snapshot conversion: the configuration rect in points is `css_rect × scale`, and
    `setSnapshotWidth(css_width)`. With `scale = 1`, `css_width` = frame width, which keeps
    today's behaviour.
  - The tests `web_schemes_stay_in_the_tab` and `retina_screenshots_shrink_to_css_size` move with
    their code.
- [ ] **Step 2:** Run `cargo test -p sawit browser`. Expected: the 5 existing tests pass.
- [ ] **Step 3:** Run `rg -n "wry|objc2|block2" crates/app/src/browser --glob '!engine.rs'`. Expected: no output.
- [ ] **Step 4:** Run `cargo build -p sawit`. Expected: success.
- [ ] **Step 5:** Commit `refactor(browser): put the web view behind an Engine seam`.

### Task 2: Page modules and the overlay

**Files:**
- Create: `crates/core/src/browser_overlay.js`
- Modify: `crates/core/src/browser_agent.js`, `crates/core/src/browser.rs`, `crates/app/src/browser/mod.rs`

**Interfaces — Produces:**
- **Rust constants and functions:**
  - `pub const OVERLAY_JS: &str = include_str!("browser_overlay.js");`
  - `pub const MODULES_PLACEHOLDER: &str = "/*__SAWIT_MODULES__*/";`
  - `pub fn page_script(token: &str) -> String`: `AGENT_JS` with `[OVERLAY_JS]` joined by `\n`
    at the placeholder, then the token replaced. Plan 3 appends `DESIGN_JS` to that list.
- **New `PageEvent` variants:**
  - `Picked(Value)`: IPC kind `picked`, payload `{"mode":"annotate"|"inspect","element":{selector,label,tag,id,classes,text,rect:{x,y,width,height}}}`.
    Plan 2 extends `element`.
  - `Hovered(Value)`: kind `hovered`, payload `{selector,label,rect}`. Sent in inspect mode
    only, at most every 100 ms.
  - `Key(String)`: kind `key`, payload `"toggle-annotate"` or `"escape"`.
- **Page ops** (through `__sawitAgent.run`):

  | Op | Answer |
  |---|---|
  | `{"op":"overlay","hidden":bool}` | `{hidden}` |
  | `{"op":"pick","mode":"annotate"\|"inspect"\|"off"}` | `{mode}` |
  | `{"op":"markers","items":[{"n":1,"selector":"…"}]}` | `{placed:[n…],missing:[n…]}` (replaces all markers) |
  | `{"op":"flash","selector":"…"}` | `{found}` (scrolls into view at `block:"center"`, outline pulses 600 ms) |

- **`BrowserView`:** `last_pick: Option<String>` (selector), set on `Picked`. `Hovered` is
  ignored until Plan 3. `Key` is handled in Task 4.

- [ ] **Step 1: Write failing tests** in `crates/core/src/browser.rs`:
  ```rust
  #[test]
  fn page_script_splices_the_modules() {
      let s = page_script("abc123");
      assert!(!s.contains(MODULES_PLACEHOLDER) && !s.contains(TOKEN_PLACEHOLDER));
      assert!(s.contains("\"abc123\""));
      assert_eq!(s.matches("Object.defineProperty(window, \"__sawitAgent\"").count(), 1);
      assert!(s.find("sawit-overlay").unwrap() < s.find("const OPS").unwrap());
  }
  #[test]
  fn ipc_reads_picks_hovers_and_keys() {
      let picked = parse_ipc("tok\npicked\n{\"mode\":\"annotate\",\"element\":{\"selector\":\"#a\"}}", "tok");
      assert!(matches!(picked, Some(PageEvent::Picked(v)) if v["element"]["selector"] == "#a"));
      assert_eq!(parse_ipc("tok\nkey\n\"toggle-annotate\"", "tok"), Some(PageEvent::Key("toggle-annotate".into())));
      assert_eq!(parse_ipc("bad\nkey\n\"escape\"", "tok"), None);
  }
  ```
- [ ] **Step 2:** Run `cargo test -p sawit-core browser`. Expected: FAIL (`page_script` missing).
- [ ] **Step 3: Agent changes.**
  - Before `const OPS` insert `const modules = {}; const helpers = {};` and the placeholder
    line, and spread `...modules` last into `OPS`.
  - The snapshot walker skips elements with `data-sawit-overlay`.
  - Modules may use the agent's `post`, `resolve(op)` and `describe(el)`. They share functions
    through `helpers` (the overlay registers `helpers.selectorFor`).
  - The agent isn't strict mode, so modules define functions as `const` arrows, never as
    function declarations inside their block.
- [ ] **Step 4: Write `browser_overlay.js`.**
  - It is a bare block ending in `Object.assign(modules, { overlay, pick, markers, flash })`.
  - Header comment: "Picker and selector logic adapted from Visual Annotator (MIT License, ©
    2026 wernerstrauch)".
  - Host and visibility as in Global Constraints.
  - `selectorFor(el)`, adapted from Visual Annotator's `genSelector`:
    1. `#id` when valid.
    2. Else `tag.classes` when that selector is unique.
    3. Else the `tag:nth-of-type(n)` path up to an id or `body`.
  - **Picking:** listeners on `window` with `{capture: true}`.
    - `pointermove`/`mousemove` move the highlight.
    - `pointerdown`, `pointerup`, `mousedown`, `mouseup`, `click`, `auxclick` and
      `contextmenu` get `preventDefault()` + `stopImmediatePropagation()`. `click` selects
      and posts `picked`.
    - `keydown`: Esc posts `escape` and turns picking off. ↑ goes to the parent, ↓ back down
      the walked path.
    - `wheel` with `altKey` walks too (and is prevented).
  - **Hit test:** `document.elementsFromPoint`, skipping the host and descending into open
    shadow roots.
  - **Highlight:**
    - Margin box filled `rgba(246,178,107,.35)`, padding box `rgba(147,196,125,.35)`, content
      outline `2px solid #0a84ff`.
    - Label chip: `tag#id.class W×H` in 11px system-ui, white on `#1d1d1f`.
  - **Markers:** 20 px `#0a84ff` circles with a white bold number at the element's top-left
    (−10 px). They are re-placed on `scroll`/`resize` (passive, rAF) and every 500 ms.
  - **Shortcut:** an always-on `keydown` capture listener: ⌘⇧. (`metaKey && shiftKey && code
    === "Period"`) posts `toggle-annotate` and calls `preventDefault()`.
  - `pick off` removes every picking listener and the highlight.
- [ ] **Step 5:** In `BrowserView`, build the web view with `page_script(&self.token)`. Handle
  `Picked` → `last_pick`.
- [ ] **Step 6:** Run `node --check crates/core/src/browser_agent.js && node --check crates/core/src/browser_overlay.js && cargo test -p sawit-core browser && cargo build -p sawit`. Expected: no output from node; tests pass; build succeeds.
- [ ] **Step 7:** Commit `feat(browser): overlay module for picking, highlights and markers`.

### Task 3: Device sizes

**Files:**
- Modify: `crates/core/src/browser.rs`, `crates/app/src/browser/mod.rs`, `crates/app/src/browser/engine.rs`

**Interfaces — Produces** (core):
- `pub struct Device { pub name: &'static str, pub width: u32, pub height: u32 }`
- `pub const DEVICES: &[Device]` (the six from Global Constraints, in that order).
- `pub fn device_by_name(name: &str) -> Option<&'static Device>`: case-insensitive, ignores `″`
  and `"`.
- `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)] pub enum ViewportMode { #[default] Responsive, Fixed { name: Option<String>, width: u32, height: u32 } }`
- `pub struct Fit { pub scale: f64, pub width: f32, pub height: f32 }`
- `pub fn fit(size: (u32, u32), area: (f32, f32)) -> Fit`: `scale = min(1, aw/w, ah/h)`.
- `pub fn resize_mode(device: Option<&str>, width: Option<u32>, height: Option<u32>) -> Result<ViewportMode, String>`
- `BrowserOp::Resize { device: Option<String>, width: Option<u32>, height: Option<u32> }` (native).

**Interfaces — Produces** (app):
- `const DEVICE_PADDING: f32 = 16.;`, `const CAPTION_BAND: f32 = 24.;`
- `fn page_frame(bounds: Frame, mode: &ViewportMode) -> (Frame, f64)`
- `BrowserView.viewport: ViewportMode`.
- `describe()` adds `"viewport": {"width","height","device","scale"}`. Width and height are the
  CSS viewport.

- [ ] **Step 1: Write failing tests.**
  - Core:
    - `fit((393,852),(1000.,1000.))` → `{1.0, 393., 852.}`.
    - `fit((1440,900),(720.,900.))` → `{0.5, 720., 450.}`.
    - `fit((393,852),(393.,426.))` → scale `0.5`.
    - `device_by_name("iphone 15")`, `("IPAD PRO 11")` and `("Desktop")` are `Some`;
      `("watch")` is `None`.
    - `resize_mode(Some("responsive"),None,None) == Ok(Responsive)`.
    - `resize_mode(None,Some(800),Some(600))` → `Fixed{name:None,width:800,height:600}`.
    - `resize_mode(None,Some(100),Some(600))` errs with exactly "Width and height must be
      between 200 and 3840."
    - `resize_mode(None,None,None)` errs with exactly "Give a device (iPhone 15, Pixel 8, iPad
      mini, iPad Pro 11″, Laptop, Desktop or responsive) or a width and height."
    - `resize_mode(Some("watch"),None,None)` errs with exactly "Unknown device “watch”. Use
      iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive."
    - `BrowserOp` round-trips `{"op":"resize","device":"iPhone 15"}`.
  - App:
    - `page_frame(Frame{x:0.,y:0.,width:800.,height:1000.}, &Fixed{name:Some("iPhone 15".into()),width:393,height:852})`
      == `(Frame{x:203.5,y:86.,width:393.,height:852.}, 1.0)`.
    - `Responsive` returns the bounds and `1.0`.
- [ ] **Step 2:** Run `cargo test -p sawit-core browser && cargo test -p sawit browser`. Expected: FAIL (missing items).
- [ ] **Step 3: Implement.**
  - Device mode:
    - Usable area: bounds minus `DEVICE_PADDING` on every side and `CAPTION_BAND` on top.
    - The frame is centered in that area (y offset = `CAPTION_BAND + DEVICE_PADDING` + the
      vertical centering remainder).
    - When frame or scale change, call `engine.set_frame` and `engine.set_zoom(scale)`.
  - The area behind the page uses `cx.theme().muted`.
  - The caption is drawn centered in the band: `iPhone 15 · 393 × 852 · 75%`. Leave out the
    name when there isn't one, and the percentage at 100 %.
  - `Resize` → `resize_mode`, store the mode, notify, answer `describe()["viewport"]`.
  - Screenshots pass `css_width = viewport width` and `scale`.
- [ ] **Step 4:** Run `cargo test -p sawit-core browser && cargo test -p sawit browser && cargo build -p sawit`. Expected: pass.
- [ ] **Step 5:** Commit `feat(browser): device sizes with page zoom`.

### Task 4: Toolbar toggles, side panel, ⌘⇧. and clean screenshots

**Files:**
- Create: `crates/app/src/browser/toolbar.rs` (today's toolbar moves here), `crates/app/src/browser/panel.rs`
- Modify: `crates/app/src/browser/mod.rs`, `crates/app/src/workspace.rs` (key binding)

**Interfaces — Produces:**
- `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Panel { Annotate, Design }`
- `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum PickMode { Off, Annotate, Inspect }`,
  with `fn as_str(self) -> &'static str` returning `"off"`/`"annotate"`/`"inspect"`.
- `pub const PANEL_WIDTH: f32 = 340.;`
- `actions!(browser, [ToggleAnnotate]);`, bound to `cmd-shift-.` in key context `"Browser"`.
- **`BrowserView` methods:**
  - `pub fn toggle_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>)`:
    Annotate → `PickMode::Annotate`, Design → `PickMode::Inspect`, closing → `Off`.
  - `pub fn panel(&self) -> Option<Panel>`
  - `pub(crate) fn page_op(&self, op: Value) -> oneshot::Receiver<OpResult>`: evaluates
    `agent_call(&op)`, answers via `agent_result`. Without an engine: `Err("The browser isn't open.")`.
  - `fn set_pick(&mut self, mode: PickMode, cx: &mut Context<Self>)`
  - `fn ops_after_load(&self) -> Vec<Value>`: run on `Load { started: false }`.
  - `fn overlay_op(hidden: bool) -> Value`
  - `fn shoot(&mut self, css_rect: Option<(f64,f64,f64,f64)>, reply: oneshot::Sender<OpResult>, cx)`:
    hide overlay → snapshot → show overlay. Used by every screenshot path.
- `panel.rs`: `pub fn frame(title: &str, body: impl IntoElement, on_close: …, cx) -> impl IntoElement`.
  Header with title and a ghost `X` button; the body scrolls.
- `describe()` adds `"panel"`, `"pick_mode"` and `"last_pick"`.

- [ ] **Step 1: Write failing tests** in `browser/mod.rs`:
  - `ops_after_load_rearms_pick`: with `PickMode::Off` → `vec![]`; with `Annotate` →
    `vec![json!({"op":"pick","mode":"annotate"})]`.
  - `screenshot_wraps_overlay`: `overlay_op(true) == json!({"op":"overlay","hidden":true})`,
    and the `shoot` sequence helper `shoot_ops()` returns `[overlay_op(true), overlay_op(false)]`.
- [ ] **Step 2:** Run `cargo test -p sawit browser`. Expected: FAIL.
- [ ] **Step 3: Toolbar (`toolbar.rs`).** After the address field, in order:
  1. **Device menu:** Lucide `Smartphone`, `DropdownMenu` as in `crates/app/src/git/panel.rs`.
     Items: Responsive, each `DEVICES` entry as `iPhone 15 · 393 × 852`, a separator, then
     Rotate (swaps width/height; disabled in Responsive). The current item is checked.
  2. **Annotate:** Lucide `Pencil`, tooltip "Annotate (⌘⇧.)", `.selected(panel == Annotate)`.
  3. **Design:** Lucide `Palette`, tooltip "Design", `.selected(panel == Design)`.
  4. **Open externally:** Lucide `ExternalLink`, tooltip "Open in Default Browser",
     `cx.open_url(current_url)`, disabled on `about:blank`.
  5. **Web Inspector:** the existing `Code` button.
- [ ] **Step 4: Panel and keys.**
  - Render `h_flex(page area, panel)` when a panel is open. The panel bodies are placeholder
    text: Annotate "Click an element on the page to annotate it."; Design "Inspect, colors,
    fonts, assets and the design system appear here." Plans 2 and 3 replace them.
  - `Key("toggle-annotate")` and the `ToggleAnnotate` action → `toggle_panel(Annotate)`.
  - `Key("escape")` → `set_pick(Off)`; the panel stays open.
  - When the `pick` op fails because the page isn't ready, set `pick_waiting = true`. The
    Annotate / Design tooltip then reads "Waiting for the page…" until `ops_after_load` re-arms
    it successfully.
- [ ] **Step 5:** Route `Screenshot` (whole view and element) through `shoot`. The overlay is
  shown again on every outcome.
- [ ] **Step 6:** Run `cargo test -p sawit browser && cargo build -p sawit`. Expected: pass.
- [ ] **Step 7:** Commit `feat(browser): toolbar toggles, side panel, ⌘⇧. and clean screenshots`.

### Task 5: `browser_resize`, app_state, Ports → Browser

**Files:**
- Modify: `crates/mcp/src/browser.rs`, `crates/mcp/src/lib.rs`, `crates/app/src/ports.rs`, `crates/app/src/workspace.rs`

**Interfaces — Consumes:** `BrowserOp::Resize`, `resize_mode` (Task 3).
**Produces:**
- `pub enum PortsEvent { OpenInBrowser(String) }` with `impl EventEmitter<PortsEvent> for PortsPanel {}`.
- Tool `browser_resize(device?, width?, height?, project?)`.

- [ ] **Step 1: Write failing tests.**
  - `lib.rs`'s tool-name list test now includes `"browser_resize"`.
  - In `mcp/src/browser.rs`: `resize_args_need_a_device_or_size`. `ResizeArgs` with nothing set
    errors with the core message from Task 3.
- [ ] **Step 2:** Run `cargo test -p sawit-mcp`. Expected: FAIL.
- [ ] **Step 3: The tool.**
  - Description: "Set the browser tab's viewport: a device (iPhone 15, Pixel 8, iPad mini, iPad
    Pro 11″, Laptop, Desktop), a width and height in CSS px, or responsive to fill the tab
    again".
  - It answers the pretty viewport JSON.
  - Add to the INSTRUCTIONS `browser_*` line: "resize to a device".
- [ ] **Step 4: Ports.**
  - A port row click emits `OpenInBrowser("http://localhost:{port}")`. With
    `event.modifiers().platform` it calls `cx.open_url` instead.
  - Tooltip: "Listening on {addr}:{port}. Click to open in Sawit's browser; ⌘-click for your
    default browser."
  - The workspace subscribes where it creates `ports_panel` (`workspace.rs:277`) and calls
    `open_browser(Some(url), window, cx)`.
- [ ] **Step 5:** Run `cargo test -p sawit-mcp && cargo build -p sawit -p sawit-mcp`. Expected: pass.
- [ ] **Step 6:** Commit `feat(browser): browser_resize, viewport in app_state, ports open in the browser`.

### Task 6: End-to-end check

**Files:**
- Create: `docs/fixtures/browser/pick.html`. It has:
  - a `#target` button;
  - `document.addEventListener("click", () => console.log("page saw click"), true)`;
  - 3 000 px of content.
- Create: `docs/fixtures/browser/hydrate.html`. It is React 18.3.1 from `https://esm.sh`, with
  `hydrateRoot(document, <App/>)` over server markup written without whitespace between tags.
  `App` renders a `<button id="count">`. This needs network.

- [ ] **Step 1:** Build and launch the dev app with the recipe in memory `visual-verify-sawit`.
  Serve the fixtures with `python3 -m http.server 8765 -d docs/fixtures/browser`.
- [ ] **Step 2: Through `sawit-mcp` over stdio**, check:
  1. `browser_navigate localhost:8765/pick.html`.
  2. `browser_press_key "Meta+Shift+."`, then `app_state`: `browser.panel == "annotate"` and
     `pick_mode == "annotate"`.
  3. `browser_click selector="#target"`: `app_state.browser.last_pick == "#target"`, and
     `browser_console pattern="page saw click"` is empty.
  4. `browser_navigate` to the same page: `pick_mode` is still `"annotate"`.
  5. `browser_resize device="iPhone 15"` with the window at 900×700:
     `browser_evaluate "window.innerWidth"` is `393`.
  6. `browser_hover selector="#target"`, then `browser_screenshot`. The file is written, and
     `browser_evaluate "document.querySelector('sawit-overlay')?.style.display ?? ''"` is `""`.
  7. `browser_navigate localhost:8765/hydrate.html`, press ⌘⇧., wait 1.5 s, hover `#count`,
     reload. `browser_console level="error"` has nothing matching `hydrat|did not match`.
- [ ] **Step 3:** Take a window screenshot (memory recipe) and check: the toolbar order, the
  Annotate button highlighted, the panel at 340 px, and the device caption.
- [ ] **Step 4:** Commit the fixtures and any fixes: `test(browser): foundation fixtures and fixes`.
