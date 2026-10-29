//! Brainstorming canvas: Excalidraw (MIT; web/canvas, bundled into
//! `crates/app/canvas`) in a WKWebView laid over the tab's bounds. GPUI
//! can't draw web content, so the webview is a native subview of the window:
//! we keep its frame on the element's bounds and hide it whenever the tab
//! isn't on screen. Fully offline: every file is served from the binary.
//!
//! Each project has its own board, autosaved to `.nyawit/canvas.excalidraw`
//! in the project folder (the format excalidraw.com opens); exports land in
//! `.nyawit/exports/`. Without a project the board lives in the data
//! directory and exports go to ~/Downloads. The shape library is personal:
//! `canvas.excalidrawlib` in the data directory.
//!
//! The webview takes the keyboard while it is clicked; a click anywhere in
//! GPUI hands it back ([`CanvasView::release_keyboard`]), and the app's own
//! shortcuts pressed while drawing are dispatched to the window.
//!
//! The page follows Sawit's design system: [`theme_tokens`] reads the active
//! GPUI theme (colors, fonts, radii, shadows, light/dark) and the page maps
//! it onto Excalidraw's CSS variables; theme changes are sent as they happen.
//!
//! `sawit-mcp` drives the board through [`CanvasView::apply`]: each op is
//! handed to `window.sawitCanvas.apply` and answered over IPC.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{LazyLock, Mutex, mpsc};

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::*;
use include_dir::{Dir, include_dir};
use sawit_core::instances::AppPaths;
use sawit_core::nyawit;
use serde_json::{Value, json};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{Request, Response, header::CONTENT_TYPE};
use wry::{NewWindowResponse, Rect, WebView, WebViewBuilder};

use crate::states::Placeholder;

static BUNDLE: Dir = include_dir!("$CARGO_MANIFEST_DIR/canvas");

const PROTOCOL: &str = "sawitcanvas";
const ORIGIN: &str = "sawitcanvas://localhost";
const SCENE_URL: &str = "/scene.excalidraw";
const LIBRARY_URL: &str = "/library.excalidrawlib";
/// Pele, for Mermaid → shapes: the copy the app renders diagrams with.
const PELE_URL: &str = "/pele.js";

/// Asks the workspace to fill the window with the canvas (or put the layout back).
pub struct ToggleCanvasMaximize;

/// What the page sends besides saves.
enum FromPage {
    Maximize,
    Key(String),
    /// The page's script has run: `window.sawitCanvas` exists.
    Ready,
    /// The answer to an [`CanvasView::apply`] op: its id and JSON result.
    Result(String, String),
}

type OpResult = Result<Value, String>;

pub struct CanvasView {
    /// The project whose board this is; `None` for the app-wide one.
    pub project: Option<PathBuf>,
    webview: Option<WebView>,
    /// The window the webview is in, for the shortcuts it passes on.
    window: Option<AnyWindowHandle>,
    error: Option<SharedString>,
    bounds: Option<Bounds<Pixels>>,
    /// The tab is active and the center pane is showing.
    visible: bool,
    maximized: bool,
    /// Sawit's design tokens as last sent to the page (JSON).
    theme: String,
    to_writer: mpsc::Sender<(PathBuf, String)>,
    to_view: UnboundedSender<FromPage>,
    /// Ops sent before the page is ready, in order.
    queued: Vec<String>,
    /// The page has loaded and takes ops.
    ready: bool,
    pending: HashMap<String, oneshot::Sender<OpResult>>,
    next_op: u64,
    _ipc: Task<()>,
    _theme: Subscription,
}

impl EventEmitter<ToggleCanvasMaximize> for CanvasView {}

impl CanvasView {
    pub fn new(project: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let (to_view, mut from_page) = unbounded();
        let ipc = cx.spawn(async move |this, cx| {
            while let Some(message) = from_page.next().await {
                if let FromPage::Key(key) = &message {
                    // An app shortcut pressed while drawing (e.g. ⌘J): the
                    // app takes the keyboard back and handles it.
                    let Ok(keystroke) = Keystroke::parse_with_os(key, OperatingSystem::Mac) else {
                        continue;
                    };
                    let Ok(Some(window)) = this.update(cx, |this, _| {
                        this.release_keyboard();
                        this.window
                    }) else {
                        break;
                    };
                    let _ = cx.update_window(window, |_, window, cx| {
                        window.dispatch_keystroke(keystroke, cx);
                    });
                    continue;
                }
                let alive = this.update(cx, |this, cx| match message {
                    FromPage::Maximize => cx.emit(ToggleCanvasMaximize),
                    FromPage::Key(_) => {}
                    FromPage::Ready => {
                        this.ready = true;
                        if let Some(webview) = &this.webview {
                            for script in this.queued.drain(..) {
                                let _ = webview.evaluate_script(&script);
                            }
                        }
                    }
                    FromPage::Result(id, json) => {
                        if let Some(reply) = this.pending.remove(&id) {
                            let _ = reply.send(op_result(&json));
                        }
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            project,
            webview: None,
            window: None,
            error: None,
            bounds: None,
            visible: false,
            maximized: false,
            theme: theme_tokens(cx).to_string(),
            to_writer: spawn_writer(),
            to_view,
            queued: Vec::new(),
            ready: false,
            pending: HashMap::new(),
            next_op: 0,
            _ipc: ipc,
            _theme: cx.observe_global::<Theme>(|this, cx| this.sync_theme(cx)),
        }
    }

    /// Sends Sawit's theme to the page again if it changed.
    fn sync_theme(&mut self, cx: &App) {
        let theme = theme_tokens(cx).to_string();
        if theme == self.theme {
            return;
        }
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script(&format!("window.sawitCanvas?.setTheme({theme})"));
        }
        self.theme = theme;
    }

    /// Runs a canvas op (see `web/canvas/src/main.tsx`) once the page is
    /// ready; the receiver gets the page's answer.
    pub fn apply(&mut self, op: Value) -> oneshot::Receiver<OpResult> {
        let (tx, rx) = oneshot::channel();
        if let Some(e) = &self.error {
            let _ = tx.send(Err(format!("The canvas couldn't start: {e}")));
            return rx;
        }
        self.next_op += 1;
        let id = self.next_op.to_string();
        self.pending.retain(|_, reply| !reply.is_canceled());
        self.pending.insert(id.clone(), tx);
        let script = format!(
            "window.sawitCanvas.apply({}, {op})",
            Value::String(id.clone())
        );
        match self.webview.as_ref().filter(|_| self.ready) {
            Some(webview) => {
                if let Err(e) = webview.evaluate_script(&script) {
                    let _ = self
                        .to_view
                        .unbounded_send(FromPage::Result(id, error_json(&e.to_string())));
                }
            }
            None => self.queued.push(script),
        }
        rx
    }

    /// Shows or hides the webview; it sits above GPUI's drawing, so it must
    /// be hidden whenever another tab or pane takes its place.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if let Some(webview) = &self.webview {
            let _ = webview.set_visible(visible);
            if visible {
                let _ = webview.focus();
            } else {
                // Save straight away instead of after the debounce, and give
                // the keyboard back to the app.
                let _ = webview.evaluate_script("window.sawitCanvas?.flush()");
                let _ = webview.focus_parent();
            }
        }
    }

    /// Gives the keyboard back to GPUI. The webview keeps it after a click
    /// otherwise, and the terminal or an editor beside it can't be typed in.
    pub fn release_keyboard(&self) {
        if let Some(webview) = &self.webview {
            let _ = webview.focus_parent();
        }
    }

    pub fn set_maximized(&mut self, maximized: bool) {
        if self.maximized == maximized {
            return;
        }
        self.maximized = maximized;
        if let Some(webview) = &self.webview {
            let _ =
                webview.evaluate_script(&format!("window.sawitCanvas?.setMaximized({maximized})"));
        }
    }

    fn place(&mut self, bounds: Bounds<Pixels>, window: &Window) {
        if self.error.is_some() {
            return;
        }
        self.window = Some(window.window_handle());
        let Some(webview) = &self.webview else {
            match self.build(bounds, window) {
                Ok(webview) => self.webview = Some(webview),
                Err(e) => {
                    self.error = Some(e.to_string().into());
                    self.queued.clear();
                    for (_, reply) in self.pending.drain() {
                        let _ = reply.send(Err(format!("The canvas couldn't start: {e}")));
                    }
                }
            }
            self.bounds = Some(bounds);
            return;
        };
        if self.bounds != Some(bounds) {
            self.bounds = Some(bounds);
            let _ = webview.set_bounds(rect(bounds));
        }
    }

    fn build(&self, bounds: Bounds<Pixels>, window: &Window) -> wry::Result<WebView> {
        let writer = self.to_writer.clone();
        let to_view = self.to_view.clone();
        let maximized = self.maximized;
        let theme = &self.theme;
        let background = rgba_tuple(&self.theme);
        let downloaded = Rc::new(RefCell::new(None::<PathBuf>));
        let download_done = downloaded.clone();
        let scene = self.scene_file();
        let save_to = scene.clone();
        let project = self.project.clone();
        // `.nyawit/` is made on the first save only: the IPC handler runs on
        // the main thread, and saves come every few hundred ms while drawing.
        let ensured = std::cell::Cell::new(false);
        let export_project = self.project.clone();
        WebViewBuilder::new()
            .with_custom_protocol(PROTOCOL.into(), move |_, request| serve(request, &scene))
            .with_url(format!("{ORIGIN}/index.html"))
            .with_ipc_handler(move |request| {
                let body = request.body();
                if let Some(scene) = body.strip_prefix("save\n") {
                    if let Some(project) = &project
                        && !ensured.get()
                    {
                        ensured.set(nyawit::ensure(project, &nyawit::dir(project)).is_ok());
                    }
                    let _ = writer.send((save_to.clone(), scene.to_owned()));
                } else if let Some(library) = body.strip_prefix("library\n") {
                    let _ = writer.send((library_file(), library.to_owned()));
                } else if body == "maximize" {
                    let _ = to_view.unbounded_send(FromPage::Maximize);
                } else if let Some(key) = body.strip_prefix("key\n") {
                    let _ = to_view.unbounded_send(FromPage::Key(key.to_owned()));
                } else if body == "ready" {
                    let _ = to_view.unbounded_send(FromPage::Ready);
                } else if let Some(rest) = body.strip_prefix("result\n")
                    && let Some((id, json)) = rest.split_once('\n')
                {
                    let _ =
                        to_view.unbounded_send(FromPage::Result(id.to_owned(), json.to_owned()));
                }
            })
            // Links (help, embeds) open in the browser; the canvas page stays put.
            .with_navigation_handler(|url| {
                if url.starts_with(ORIGIN) || url.starts_with("blob:") || url.starts_with("data:") {
                    return true;
                }
                open_externally(&url);
                false
            })
            .with_new_window_req_handler(|url, _| {
                open_externally(&url);
                NewWindowResponse::Deny
            })
            // "Export image" / "Save to…" download to the project's
            // `.nyawit/exports/` (else ~/Downloads, wry's default spot);
            // reveal the file when done.
            .with_download_started_handler(move |_, path| {
                if let Some(project) = &export_project
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                {
                    let exports = nyawit::exports_dir(project);
                    if nyawit::ensure(project, &exports).is_ok() {
                        *path = nyawit::unique(&exports, name);
                    }
                }
                *downloaded.borrow_mut() = Some(path.clone());
                true
            })
            .with_download_completed_handler(move |_, _, ok| {
                if ok && let Some(path) = download_done.borrow_mut().take() {
                    let _ = std::process::Command::new("open")
                        .arg("-R")
                        .arg(path)
                        .spawn();
                }
            })
            .with_initialization_script(format!(
                "window.__sawitCanvasMaximized = {maximized}; window.__sawitTheme = {theme}; \
                 window.__sawitAppKeys = {keys}; \
                 addEventListener('DOMContentLoaded', () => window.ipc.postMessage('ready'));",
                keys = json!(crate::workspace::canvas_passthrough_keys()),
            ))
            // The app's background, until the page paints.
            .with_background_color(background)
            .with_accept_first_mouse(true)
            .with_focused(self.visible)
            .with_visible(self.visible)
            .with_bounds(rect(bounds))
            .build_as_child(window)
    }

    fn scene_file(&self) -> PathBuf {
        match &self.project {
            Some(project) => nyawit::canvas_file(project),
            None => AppPaths::default_macos().root.join("canvas.excalidraw"),
        }
    }
}

impl Drop for CanvasView {
    fn drop(&mut self) {
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script("window.sawitCanvas?.flush()");
            let _ = webview.focus_parent();
        }
    }
}

impl Render for CanvasView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(error) = &self.error {
            return Placeholder::error("The canvas couldn't start")
                .detail(error.clone())
                .into_any_element();
        }
        let this = cx.entity().downgrade();
        div()
            .size_full()
            .child(
                canvas(
                    {
                        let this = this.clone();
                        move |bounds, window, cx| {
                            let _ = this.update(cx, |this, _| this.place(bounds, window));
                        }
                    },
                    // GPUI only sees clicks outside the webview, which sits on
                    // top: each one takes the keyboard back from it.
                    move |_, _, window, _| {
                        let this = this.clone();
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = this.update(cx, |this, _| this.release_keyboard());
                            }
                        });
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }
}

/// `{"ok": true, …}` is the op's result; `{"ok": false, "error": …}` its failure.
fn op_result(json: &str) -> OpResult {
    let mut value: Value =
        serde_json::from_str(json).map_err(|e| format!("The canvas sent a bad answer: {e}"))?;
    let ok = value.get("ok").and_then(Value::as_bool) == Some(true);
    if !ok {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("The canvas op failed.")
            .to_string());
    }
    if let Some(map) = value.as_object_mut() {
        map.remove("ok");
    }
    Ok(value)
}

fn error_json(message: &str) -> String {
    serde_json::json!({ "ok": false, "error": message }).to_string()
}

/// The design tokens the page maps onto Excalidraw (see web/canvas/src/theme.ts).
fn theme_tokens(cx: &App) -> Value {
    let theme = cx.theme();
    let c = &theme.colors;
    json!({
        "mode": if theme.mode.is_dark() { "dark" } else { "light" },
        "font": theme.font_family.as_ref(),
        "monoFont": theme.mono_font_family.as_ref(),
        "radius": f32::from(theme.radius),
        "radiusLg": f32::from(theme.radius_lg),
        "shadow": theme.shadow,
        "colors": {
            "background": hex(c.background),
            "foreground": hex(c.foreground),
            "border": hex(c.border),
            "muted": hex(c.muted),
            "mutedForeground": hex(c.muted_foreground),
            "popover": hex(c.popover),
            "popoverForeground": hex(c.popover_foreground),
            "primary": hex(c.primary),
            "primaryForeground": hex(c.primary_foreground),
            "primaryHover": hex(c.primary_hover),
            "primaryActive": hex(c.primary_active),
            "secondary": hex(c.secondary),
            "secondaryHover": hex(c.secondary_hover),
            "secondaryActive": hex(c.secondary_active),
            "input": hex(c.input),
            "ring": hex(c.ring),
            "selection": hex(c.selection),
            "link": hex(c.link),
            "danger": hex(c.danger),
            "dangerForeground": hex(c.danger_foreground),
            "scrollbarThumb": hex(c.scrollbar_thumb),
            "scrollbarThumbHover": hex(c.scrollbar_thumb_hover),
        },
    })
}

/// `#rrggbb`, or `#rrggbbaa` when translucent.
fn hex(color: Hsla) -> String {
    let c = Rgba::from(color);
    let [r, g, b, a] = [c.r, c.g, c.b, c.a].map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// The tokens' background as the webview's (r, g, b, a), shown until the
/// page paints.
fn rgba_tuple(tokens: &str) -> (u8, u8, u8, u8) {
    let background = serde_json::from_str::<Value>(tokens)
        .ok()
        .and_then(|t| t["colors"]["background"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let byte = |i: usize| u8::from_str_radix(background.get(i..i + 2)?, 16).ok();
    match (byte(1), byte(3), byte(5)) {
        (Some(r), Some(g), Some(b)) => (r, g, b, 0xff),
        _ => (0x28, 0x2c, 0x33, 0xff),
    }
}

fn rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        position: LogicalPosition::new(f32::from(bounds.origin.x), f32::from(bounds.origin.y))
            .into(),
        size: LogicalSize::new(
            f32::from(bounds.size.width).max(1.),
            f32::from(bounds.size.height).max(1.),
        )
        .into(),
    }
}

fn library_file() -> PathBuf {
    AppPaths::default_macos().root.join("canvas.excalidrawlib")
}

fn open_externally(url: &str) {
    if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:") {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

/// Writes saves on its own thread, only the newest queued for each file,
/// through a temporary file so a crash mid-write can't truncate the board.
fn spawn_writer() -> mpsc::Sender<(PathBuf, String)> {
    let (tx, rx) = mpsc::channel::<(PathBuf, String)>();
    std::thread::Builder::new()
        .name("canvas-save".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut latest = vec![first];
                while let Ok((file, body)) = rx.try_recv() {
                    latest.retain(|(f, _)| *f != file);
                    latest.push((file, body));
                }
                for (file, body) in latest {
                    let mut tmp = file.clone().into_os_string();
                    tmp.push(".tmp");
                    if let Some(dir) = file.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if std::fs::write(&tmp, &body).is_ok() {
                        keep_backup_before_emptying(&file, &body);
                        let _ = std::fs::rename(&tmp, &file);
                    }
                }
            }
        })
        .expect("spawn canvas writer");
    tx
}

/// A save that empties a board which had elements keeps the old board as
/// `<file>.bak`, so "Clear canvas" (or an agent's `clear`) can be undone
/// even after the app restarts.
fn keep_backup_before_emptying(file: &std::path::Path, body: &str) {
    if !is_empty_scene(body) {
        return;
    }
    if let Ok(old) = std::fs::read_to_string(file)
        && !old.trim().is_empty()
        && !is_empty_scene(&old)
    {
        let mut bak = file.to_path_buf().into_os_string();
        bak.push(".bak");
        let _ = std::fs::write(bak, old);
    }
}

fn is_empty_scene(json: &str) -> bool {
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|v| v.get("elements")?.as_array().map(|e| e.is_empty()))
        .unwrap_or(false)
}

fn serve(request: Request<Vec<u8>>, scene: &Path) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    let saved = match path {
        SCENE_URL => Some(scene.to_path_buf()),
        LIBRARY_URL => Some(library_file()),
        _ => None,
    };
    if let Some(file) = saved {
        let body = std::fs::read(file).unwrap_or_default();
        return respond(200, "application/json", Cow::Owned(body));
    }
    if path == PELE_URL {
        let body = sawit_core::mermaid::PELE_JS.as_bytes();
        return respond(200, mime(path), Cow::Borrowed(body));
    }
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match bundle_file(path) {
        Some(body) => respond(200, mime(path), Cow::Borrowed(body)),
        None => respond(404, "text/plain", Cow::Borrowed(b"not found")),
    }
}

/// A file of the bundle. All but the fonts are embedded as `<path>.br`
/// (web/canvas/compress.mjs, 9 MB down to under 3): each is decoded the first
/// time the page asks for it and kept for the life of the process, so only
/// what gets used (most locales and diagram chunks never are) costs memory.
fn bundle_file(path: &str) -> Option<&'static [u8]> {
    static DECODED: LazyLock<Mutex<HashMap<String, &'static [u8]>>> =
        LazyLock::new(Default::default);
    if let Some(file) = BUNDLE.get_file(path) {
        return Some(file.contents());
    }
    let packed = BUNDLE.get_file(format!("{path}.br"))?.contents();
    let mut decoded = DECODED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(body) = decoded.get(path) {
        return Some(body);
    }
    let mut body = Vec::with_capacity(packed.len() * 4);
    brotli_decompressor::BrotliDecompress(&mut &packed[..], &mut body).ok()?;
    let body: &'static [u8] = Box::leak(body.into_boxed_slice());
    decoded.insert(path.to_string(), body);
    Some(body)
}

fn respond(status: u16, mime: &str, body: Cow<'static, [u8]>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, mime)
        .body(body)
        .expect("valid response")
}

fn mime(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

/// The tab's icon and title.
pub const ICON: Lucide = Lucide::Shapes;
pub const TITLE: &str = "Canvas";

#[cfg(test)]
mod tests {
    use super::{bundle_file, hex, is_empty_scene, keep_backup_before_emptying, mime, rgba_tuple};

    #[test]
    fn emptying_a_board_keeps_a_backup() {
        let dir = std::env::temp_dir().join(format!("sawit-canvas-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("canvas.excalidraw");
        let full = r#"{"elements":[{"id":"a"}]}"#;
        let empty = r#"{"elements":[]}"#;
        std::fs::write(&file, full).unwrap();
        keep_backup_before_emptying(&file, empty);
        assert_eq!(
            std::fs::read_to_string(dir.join("canvas.excalidraw.bak")).unwrap(),
            full
        );
        // Non-empty saves don't touch the backup.
        std::fs::write(&file, empty).unwrap();
        keep_backup_before_emptying(&file, full);
        assert_eq!(
            std::fs::read_to_string(dir.join("canvas.excalidraw.bak")).unwrap(),
            full
        );
        assert!(is_empty_scene(empty) && !is_empty_scene(full) && !is_empty_scene("{}"));
        let _ = std::fs::remove_dir_all(dir);
    }
    use gpui_kit::{Hsla, Rgba};

    #[test]
    fn theme_colors_become_css_hex() {
        let opaque = Hsla::from(Rgba {
            r: 0x28 as f32 / 255.,
            g: 0x2c as f32 / 255.,
            b: 0x33 as f32 / 255.,
            a: 1.,
        });
        assert_eq!(hex(opaque), "#282c33");
        let translucent = Hsla::from(Rgba {
            r: 0x74 as f32 / 255.,
            g: 0xad as f32 / 255.,
            b: 0xe8 as f32 / 255.,
            a: 0x3d as f32 / 255.,
        });
        assert_eq!(hex(translucent), "#74ade83d");
    }

    #[test]
    fn webview_background_comes_from_the_tokens() {
        assert_eq!(
            rgba_tuple(r##"{"colors":{"background":"#fafafa"}}"##),
            (0xfa, 0xfa, 0xfa, 0xff)
        );
        // Unparseable: One Dark's background.
        assert_eq!(rgba_tuple("{}"), (0x28, 0x2c, 0x33, 0xff));
    }

    #[test]
    fn bundle_has_the_page_and_its_assets() {
        for path in [
            "index.html",
            "canvas.js",
            "canvas.css",
            "fonts/Virgil/Virgil-Regular.woff2",
            "fonts/Cascadia/CascadiaCode-Regular.woff2",
        ] {
            assert!(
                bundle_file(path).is_some_and(|b| !b.is_empty()),
                "missing {path}"
            );
        }
    }

    #[test]
    fn every_compressed_file_decodes() {
        fn walk(dir: &include_dir::Dir<'static>, found: &mut usize) {
            for file in dir.files() {
                if let Some(path) = file.path().to_str().unwrap().strip_suffix(".br") {
                    assert!(
                        bundle_file(path).is_some_and(|b| !b.is_empty()),
                        "{path} doesn't decode"
                    );
                    *found += 1;
                }
            }
            dir.dirs().for_each(|d| walk(d, found));
        }
        let mut found = 0;
        walk(&super::BUNDLE, &mut found);
        // ~78 since mermaid.js gave way to Pele and the locales were stubbed out.
        assert!(found > 50, "only {found} compressed files");
    }

    #[test]
    fn op_results() {
        use super::op_result;
        assert_eq!(
            op_result(r#"{"ok":true,"added":2}"#),
            Ok(serde_json::json!({"added": 2}))
        );
        assert_eq!(
            op_result(r#"{"ok":false,"error":"Parse error on line 1"}"#),
            Err("Parse error on line 1".into())
        );
        assert!(op_result("nope").is_err());
    }

    #[test]
    fn module_scripts_get_a_javascript_type() {
        // WebKit refuses to run `<script type="module">` served as anything else.
        assert!(mime("canvas.js").starts_with("text/javascript"));
        assert_eq!(mime("assets/fonts/x.woff2"), "font/woff2");
    }
}
