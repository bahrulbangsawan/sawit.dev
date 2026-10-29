# Browser pane: design

A built-in browser tab that both the user and an AI agent drive, in the
spirit of Cursor's agent browser (https://cursor.com/docs/agent/tools/browser),
built on what Sawit already has: a WKWebView laid over a GPUI tab (as the
canvas does), the app control socket, and `sawit-mcp`.

## Why this approach

| Option | Rendering | Agent control | Cost |
|---|---|---|---|
| **wry / WKWebView child view (chosen)** | Native, GPU composited, zero copies | Injected script + native WebKit APIs | ~0 MB; GPUI can't draw over it |
| CEF offscreen (`wef`) | Frames copied into a GPUI texture | Full CDP | +150–200 MB, experimental |
| External Chrome over CDP, screencast | JPEG frames | Strongest | Needs Chrome, laggy |

The canvas already proves the child-view approach in this app (placement,
visibility, focus hand-off, IPC). WebKit is the engine Safari users ship to,
and it costs nothing in binary size. CDP-level features that WebKit can't
give (trusted input events, full network bodies for every sub-resource)
are the accepted trade-off.

## User-facing behaviour

- **Opening:** a "Browser" button in the title bar, `app_open_page
  page=browser`, or any `browser_*` tool. One browser tab per window
  (like the canvas).
- **Toolbar (GPUI):** back, forward, reload / stop, an address field, and an
  "Inspect" button (Safari Web Inspector). Enter in the address field
  navigates. Text that isn't a URL becomes a search (DuckDuckGo);
  `localhost:3000` and bare `host:port` get `http://`; other bare hosts get
  `https://`.
- **Empty start:** a new browser opens the project's running dev server when
  there is exactly one whose folder is inside the project, otherwise a blank
  page with "Type a URL" focus in the address field.
- **Storage:** cookies, localStorage, IndexedDB persist per project (a
  WKWebsiteDataStore per project folder, macOS 14+; the default store
  before 14).
- **Links:** `target=_blank` opens in the same tab. `mailto:` and other
  non-web schemes go to the system.
- **Visibility:** the webview is hidden whenever its tab isn't showing,
  exactly as the canvas is; it keeps running in the background.

## Agent tools (`sawit-mcp`)

All take `project` (window selection) like the other `app_*` tools.

| Tool | Does |
|---|---|
| `browser_navigate` | `url`, or `action` back / forward / reload. Opens the tab. Waits for load (≤ `timeout_ms`, default 15 s) and returns url, title, status |
| `browser_snapshot` | The page as an accessibility-style outline; each element gets a ref (`e12`) valid until the next snapshot or navigation |
| `browser_click` | By `ref` or CSS `selector`; `double`, `button` left/right. Reports when the element is covered by another |
| `browser_hover` | By ref/selector |
| `browser_type` | Text into a field by ref/selector; `clear` first, `submit` (Enter / form submit) after |
| `browser_press_key` | `Enter`, `Tab`, `Escape`, arrows, `Backspace`, single characters, with modifiers (`Meta+a`) to the focused element |
| `browser_select_option` | `<select>` by ref/selector and option value(s) or label(s) |
| `browser_scroll` | Page or element by `dx`/`dy`, or a ref into view |
| `browser_wait_for` | Until `text` appears / `text_gone` disappears / `selector` matches, or `time_ms` passes (≤ 30 s) |
| `browser_screenshot` | PNG of the visible page, or of one element (`ref`); returned as an image and written to a file (`output_path` or Sawit's cache) |
| `browser_console` | Captured console messages and page errors: `level` filter (error / warning / info / log / debug), `pattern` (substring, case-insensitive), `limit` (default 100), `clear` |
| `browser_network` | Captured fetch/XHR (method, url, status, duration, request and response bodies cut to 2 KB/4 KB) and other resources (url, type, status where WebKit reports it, duration): `pattern`, `failed_only`, `limit`, `clear` |
| `browser_evaluate` | Runs a JS expression or function body in the page and returns its JSON value |

`app_state` gains `"browser": {"url", "title", "loading"}` or `null`.

## Architecture

```
sawit-mcp browser_* ──► app_control Command::Browser{op} ──► Workspace::control
                                                               │ open/activate tab
                                                               ▼
                                                   BrowserView::apply(BrowserOp)
                       ┌───────────────────────────────────────┼────────────────────────┐
                 native (wry/WebKit)                     JS agent (evaluate_script_with_callback)
           navigate, back/forward, reload,              snapshot, click, hover, type, press,
           screenshot (takeSnapshot), state,            select, scroll, query (for wait_for),
           console/network buffers, devtools            evaluate, element rect
                       ▲
          IPC (token-checked): console, error, network, url-change ◄── init script in every page
```

- **`sawit_core::browser`** (pure, unit-tested): `BrowserOp` (serde, tag
  `op`), the JS agent source (`browser_agent.js`, `include_str!`), address
  normalisation, log entry types with the ring buffers and their filters,
  IPC message parsing, the per-project data store id, and the dev-server
  pick.
- **`crates/app/src/browser.rs`**: `BrowserView` (GPUI): toolbar, webview
  placement (shared idioms with `canvas.rs`), op dispatch, screenshot via
  `WKWebView takeSnapshotWithConfiguration`.
- **`crates/mcp/src/browser.rs`**: the tools. Waits (load, `wait_for`) are
  polling loops here over `state`/`query` ops (200 ms), so the app only
  implements instantaneous ops.

### The JS agent

Injected with `with_initialization_script` into every document:

- Captures `console.*`, `error`, `unhandledrejection`, wraps `fetch` and
  `XMLHttpRequest`, observes `PerformanceObserver('resource')`, and hooks
  `history.pushState/replaceState`, `popstate`, `hashchange` for URL
  changes. Each message goes over `window.ipc.postMessage` prefixed with a
  per-webview random token held in a closure; the app drops messages
  without it, so page scripts can't forge logs or results.
- Defines `window.__sawitAgent` non-writable, non-configurable, with
  `run(op)` returning a JSON string (`{"ok":true,…}` /
  `{"ok":false,"error":…}`), synchronously, so results come back through
  `evaluate_script_with_callback` without IPC.
- Snapshot: walks the DOM (open shadow roots included, iframes not),
  skips invisible nodes, computes role (explicit, else implicit from tag)
  and accessible name (aria-labelledby, aria-label, label[for], alt, title,
  placeholder, text), keeps interactive elements, headings, landmarks,
  images, list items and text runs; output is an indented outline
  `- button "Save" [ref=e4]` capped at 2 000 lines; refs live in a `Map`
  replaced on each snapshot.
- Click: scroll into view (`block: center`), hit-test the centre with
  `elementFromPoint`; when another element is on top, fail with what covers
  it. Dispatch pointer/mouse down/up, focus, click (+ dblclick /
  contextmenu). Synthetic events run default actions for links, buttons,
  checkboxes and labels.
- Type: focus, select-all + delete when `clear`, then
  `document.execCommand('insertText')` (fires real `beforeinput`/`input`,
  works with React/Vue controlled inputs); fall back to the native value
  setter + `input` + `change`.

## Errors the agent sees

- App not running → the same message the other app tools give.
- Ref unknown → "No element e12 — take a new browser_snapshot (refs reset
  on every snapshot and navigation)."
- Element hidden / disabled / covered → says which.
- Page mid-navigation or script blocked → "The page isn't ready yet; retry
  after browser_wait_for." (no agent in the document).
- Navigation timeout → returns the state with `"loading": true` and a note,
  not an error.

## Non-goals (this version)

- Multiple browser tabs per window; iframes in snapshots.
- Trusted (OS-level) input events, file upload, drag and drop.
- Full-page screenshots beyond the viewport.
- An origin allowlist and per-action approval inside Sawit: the MCP
  client's tool approval already gates every call.
- Linux/Windows (Sawit is macOS-only).
