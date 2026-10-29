# Browser Pane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A browser tab in Sawit that the user browses and an agent drives through `browser_*` MCP tools (navigate, snapshot, click, type, keys, select, scroll, wait, screenshot, console, network, evaluate).

**Architecture:** A WKWebView (wry, child view) laid over a GPUI tab like `canvas.rs`, with a GPUI toolbar. An injected JS agent answers synchronous ops through `evaluate_script_with_callback` and streams console/network/url events over token-checked IPC. The app control socket gains `Command::Browser { op }`; `sawit-mcp` composes waits by polling.

**Tech Stack:** Rust, GPUI (gpui-kit 0.8.1), wry 0.57 (`devtools`), objc2-web-kit / objc2-app-kit 0.3.2 (snapshot), rmcp 3.8.

**Spec:** `docs/superpowers/specs/2026-10-29-browser-pane-design.md`

## Global Constraints

- macOS only; per-project storage needs macOS 14+, falling back to the default store.
- No new web bundle: the agent is one hand-written JS file embedded with `include_str!`.
- Tool copy follows the existing `sawit-mcp` voice (short sentences, curly quotes in errors).
- One browser tab per window; ops other than `navigate` and `screenshot` don't switch tabs.
- IPC messages without the webview's token are dropped.
- Snapshot ≤ 2 000 lines; network bodies cut to 2 KB request / 4 KB response; console ring 500, network ring 300.

## Review Focus

1. A ref from an old snapshot after navigation → clear "take a new snapshot" error, never a click on a random element (Task 2: refs map reset on `pagehide`; Task 4 error copy test).
2. Agent op while the page is mid-navigation (no `__sawitAgent`) → "isn't ready" error, not a hang (Task 3: eval wrapper returns an error JSON when the agent is missing; 30 s socket timeout still applies).
3. A page forging `window.ipc.postMessage("console…")` → ignored (Task 1 `parse_ipc` test without token).
4. Address field input like `localhost:5173`, `foo bar`, `example.com/path`, `file:///…` → correct URL (Task 1 `normalize_address` tests).
5. Screenshot with the tab hidden / center pane closed → the tab is shown first, then captured (Task 3: `screenshot` activates the tab like `navigate`).

---

### Task 1: Core types (`sawit_core::browser`) and the control command

**Files:**
- Create: `crates/core/src/browser.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod browser;`), `crates/core/src/app_control.rs` (`Page::Browser`, `Command::Browser { op: Value }`)

**Interfaces — Produces:**
- `enum BrowserOp` (serde `tag = "op"`, snake_case): `Navigate{url}`, `Back`, `Forward`, `Reload`, `State`, `Snapshot`, `Click{target: Target, double: bool, button: Option<String>}`, `Hover{target}`, `Type{target: Option<Target>, text, clear: bool, submit: bool}`, `Press{key}`, `Select{target, values: Vec<String>}`, `Scroll{target: Option<Target>, dx: f64, dy: f64}`, `Query{text: Option<String>, selector: Option<String>}`, `Evaluate{script}`, `Screenshot{target: Option<Target>}`, `Console{level, pattern, limit, clear}`, `Network{pattern, failed_only, limit, clear}`, `Devtools`.
- `struct Target { #[serde(rename="ref")] reference: Option<String>, selector: Option<String> }` (flattened into ops).
- `fn BrowserOp::is_native(&self) -> bool` — handled in Rust, not the JS agent.
- `fn normalize_address(input: &str) -> String`.
- `struct ConsoleEntry { level, text, url: Option<String>, line: Option<u32>, time_ms: f64 }`, `struct NetworkEntry { method, url, status: Option<u16>, kind, duration_ms: Option<f64>, request_body, response_body, error: Option<String>, time_ms }`.
- `struct Logs { console: VecDeque<ConsoleEntry>, network: VecDeque<NetworkEntry> }` with `push_console`, `push_network` (rings 500 / 300), `console_json(level, pattern, limit) -> Value`, `network_json(pattern, failed_only, limit) -> Value`.
- `enum PageEvent { Console(ConsoleEntry), Network(NetworkEntry), Url(String) }`, `fn parse_ipc(body: &str, token: &str) -> Option<PageEvent>` — body is `"<token>\n<kind>\n<json>"`.
- `fn data_store_id(project: &Path) -> [u8; 16]` — stable (FNV-1a 64 over the path bytes, two seeds).
- `fn pick_dev_server(jobs: &[PortJob], project: &Path) -> Option<String>` — `http://localhost:<lowest port>` of the single dev job whose `cwd` is inside `project`.
- `pub const AGENT_JS: &str` (from Task 2's file).

- [ ] **Step 1: Write failing tests** in `browser.rs`: `normalize_address("localhost:5173") == "http://localhost:5173"`, `("example.com/a") == "https://example.com/a"`, `("http://x.dev") ` unchanged, `("file:///tmp/a.html")` unchanged, `("foo bar")` starts with `https://duckduckgo.com/?q=foo+bar`, `("about:blank")` unchanged, `("127.0.0.1:8080")` → `http://…`; `parse_ipc` rejects a wrong token and parses a console entry; console ring keeps the newest 500; `console_json(Some("error"), Some("TYPEerror"), 10)` filters case-insensitively; `network_json(failed_only)` keeps status ≥ 400 or `error`; `data_store_id` equal for equal paths and different for different ones; `pick_dev_server` → `None` for two candidates; `BrowserOp` round-trips `{"op":"click","ref":"e3","double":false}`; `Command::Browser` wire format `{"cmd":"browser","op":{…}}`.
- [ ] **Step 2:** `cargo test -p sawit-core browser` → fails (module missing).
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p sawit-core` → passes.
- [ ] **Step 5:** Commit `feat(browser): core ops, address, logs and ipc types`.

### Task 2: The JS agent (`crates/core/src/browser_agent.js`)

**Interfaces — Consumes:** IPC format from Task 1; op JSON from `BrowserOp`. **Produces:** `window.__sawitAgent.run(op) -> string` for the non-native ops; placeholders `__SAWIT_TOKEN__` replaced by the app.

- [ ] **Step 1:** Write the file: IIFE capturing `const post = window.ipc.postMessage.bind(window.ipc)` and the token; console/error/rejection hooks; fetch/XHR wrappers; `PerformanceObserver` for non-fetch resources; history hooks; refs `Map` reset on `pagehide`; `run` for `snapshot`, `click`, `hover`, `type`, `press`, `select`, `scroll`, `query`, `evaluate`, `rect` (element bounds for screenshots). Behaviour per the spec's "The JS agent" section.
- [ ] **Step 2:** `node --check crates/core/src/browser_agent.js` → no output.
- [ ] **Step 3:** Rust test `agent_js_has_token_placeholder_and_entry` (contains `__SAWIT_TOKEN__` and `__sawitAgent`).
- [ ] **Step 4:** Commit `feat(browser): page agent script`.

### Task 3: `BrowserView` and workspace wiring (app)

**Files:**
- Create: `crates/app/src/browser.rs`
- Modify: `crates/app/Cargo.toml` (wry `devtools`; `objc2-web-kit` features `WKWebView`, `WKSnapshotConfiguration`, `block2`, `objc2-app-kit`; `objc2-app-kit` `NSImage`, `NSImageRep`, `NSBitmapImageRep`, `NSGraphics`; `block2 = "0.6"`; `image` feature `png`), `crates/app/src/main.rs` (`mod browser;`), `crates/app/src/workspace.rs` (field `browser: Option<WeakEntity<BrowserView>>`, `open_browser`, `browser_active`, `sync_browser` called next to `sync_canvas`, title-bar "Browser" button), `crates/app/src/workspace/control.rs` (`Command::Browser`, `Page::Browser`, `describe` → `"browser"`).

**Interfaces — Consumes:** Task 1 types. **Produces:** `BrowserView::new(project: Option<PathBuf>, window, cx)`, `apply(&mut self, op: BrowserOp, cx) -> oneshot::Receiver<Result<Value, String>>`, `set_visible(bool)`, `describe() -> Value` (`url`, `title`, `loading`), `Workspace::open_browser(&mut self, window, cx)`.

- [ ] **Step 1:** Unit tests in `browser.rs`: `agent_result("\"{\\\"ok\\\":true,\\\"n\\\":1}\"") == Ok({"n":1})`, empty string → "isn't ready" error; `init_script(token)` contains the token and no placeholder.
- [ ] **Step 2:** Implement: toolbar (Button ghost xsmall for back/forward/reload/inspect, `Input` address field — Enter navigates via `normalize_address`, focusing it calls `focus_parent`), webview built on first paint like `CanvasView::place` with `with_initialization_script(init_script)`, IPC → `parse_ipc` → `Logs`/url, `with_on_page_load_handler` (loading flag; clears nothing), `with_document_title_changed_handler`, `with_new_window_req_handler` → `load_url` in place, non-web schemes → `open`, `with_data_store_identifier(data_store_id(project))`, `with_devtools(true)`. Native ops answer at once; agent ops use `evaluate_script_with_callback` with the wrapper `(() => { const a = window.__sawitAgent; return a ? a.run(OP) : "" })()`. Screenshot: optional `rect` op first, then `takeSnapshotWithConfiguration`, PNG via `NSBitmapImageRep`, downscaled to 1× with `image`, base64 in `png_base64`. Ops before the webview exists queue like the canvas.
- [ ] **Step 3:** `cargo test -p sawit browser && cargo build -p sawit` → pass.
- [ ] **Step 4:** Commit `feat(browser): browser tab with toolbar and agent ops`.

### Task 4: MCP tools (`crates/mcp/src/browser.rs`)

**Files:** Create `crates/mcp/src/browser.rs`; modify `crates/mcp/src/lib.rs` (module, router, INSTRUCTIONS line), `crates/mcp/src/app.rs` (`PAGES` += browser).

**Interfaces — Consumes:** `Command::Browser { op }`, `BrowserOp`, `pick_dev_server`, `probe::listening_jobs`.

- [ ] **Step 1:** Tests: `navigate_op(None, Some("back"))` → `BrowserOp::Back`; neither → error naming both params; `target(None, None)` → error "Give a ref (from browser_snapshot) or a CSS selector."; `wait_done(&json!({"found":true}), WaitFor::Text)`.
- [ ] **Step 2:** Implement the 13 tools from the spec table. `browser_navigate` with no url and no action opens the dev server from `pick_dev_server` or errors "Give a url…". Load wait: poll `State` every 200 ms until `loading == false` or timeout. `browser_wait_for`: poll `Query`. `browser_screenshot`: decode base64, write file, return `Content::image` + path text.
- [ ] **Step 3:** `cargo test -p sawit-mcp` → pass.
- [ ] **Step 4:** Commit `feat(mcp): browser_* tools drive the browser tab`.

### Task 5: End-to-end check in the running app

- [ ] **Step 1:** Build and launch the dev app from the worktree; serve a fixture page (`docs/fixtures/browser/form.html`: a form with a text input, select, submit button logging to console, a `fetch` to a 404).
- [ ] **Step 2:** Via `sawit-mcp` over stdio (JSON-RPC), call navigate → snapshot → type → select → click submit → console (sees the log) → network (sees the 404 as failed) → screenshot (PNG file non-empty) → evaluate `document.title`.
- [ ] **Step 3:** Window screenshot to check the toolbar visually.
- [ ] **Step 4:** Commit fixes, if any.
