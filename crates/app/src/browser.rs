//! The browser tab: a WKWebView laid over the tab below a GPUI toolbar (back,
//! forward, reload, address, Web Inspector), placed and hidden like the
//! canvas's (see `canvas.rs`). Cookies and storage are kept per project.
//!
//! Agents drive it through [`BrowserView::apply`] (`sawit-mcp`'s `browser_*`
//! tools): navigation, state, logs and screenshots are answered here; the
//! rest by the page agent (`sawit_core::browser::AGENT_JS`), injected into
//! every page, through `evaluate_script_with_callback`.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use sawit_core::browser::{
    AGENT_JS, BrowserOp, ConsoleEntry, Logs, PageEvent, TOKEN_PLACEHOLDER, Target,
    normalize_address, parse_ipc,
};
use sawit_core::probe;
use serde_json::{Value, json};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebView, WebViewBuilder};

use crate::rt;
use crate::states::Placeholder;

/// The tab's icon and title.
pub const ICON: Lucide = Lucide::Globe;
pub const TITLE: &str = "Browser";

const TOOLBAR_HEIGHT: f32 = 36.;

type OpResult = Result<Value, String>;

/// What the webview reports, from its handlers (any thread) to the view.
enum FromPage {
    Event(PageEvent),
    Load {
        started: bool,
        url: String,
    },
    Title(String),
    /// A link asked for a new window: it opens here instead.
    Open(String),
    /// An element's box is known: capture it (or the whole view).
    Shoot {
        rect: Option<(f64, f64, f64, f64)>,
        reply: oneshot::Sender<OpResult>,
    },
}

pub struct BrowserView {
    project: Option<PathBuf>,
    webview: Option<Rc<WebView>>,
    error: Option<SharedString>,
    bounds: Option<Bounds<Pixels>>,
    /// The tab is active and the center pane is showing.
    visible: bool,
    address: Entity<InputState>,
    url: String,
    title: String,
    loading: bool,
    /// Something was opened (by the user or an agent); the dev server
    /// found at start no longer replaces the blank page.
    navigated: bool,
    /// Signs page messages; page scripts can't read it.
    token: String,
    logs: Logs,
    to_view: UnboundedSender<FromPage>,
    /// Ops sent before the webview exists, in order.
    queued: Vec<(BrowserOp, oneshot::Sender<OpResult>)>,
    _ipc: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl BrowserView {
    /// A browser at `url`; without one, the project's dev server when one is
    /// running, else a blank page.
    pub fn new(
        project: Option<PathBuf>,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let address =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter address"));
        let subscriptions = vec![cx.subscribe_in(
            &address,
            window,
            |this, address, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let typed = address.read(cx).value().to_string();
                    this.navigate(&typed, cx);
                }
                // Typing goes to the address field, not the page.
                InputEvent::Focus => {
                    if let Some(webview) = &this.webview {
                        let _ = webview.focus_parent();
                    }
                }
                _ => {}
            },
        )];
        let (to_view, mut from_page) = unbounded();
        let ipc = cx.spawn_in(window, async move |this, cx| {
            while let Some(message) = from_page.next().await {
                let alive = this.update_in(cx, |this, window, cx| {
                    this.receive(message, window, cx);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        let navigated = url.is_some();
        if !navigated && let Some(project) = project.clone() {
            let jobs = rt::spawn(cx, probe::listening_jobs());
            cx.spawn(async move |this, cx| {
                let Ok(Ok(jobs)) = jobs.await else { return };
                let Some(url) = sawit_core::browser::pick_dev_server(&jobs, &project) else {
                    return;
                };
                let _ = this.update(cx, |this, cx| {
                    if !this.navigated {
                        this.navigate(&url, cx);
                    }
                });
            })
            .detach();
        }
        Self {
            project,
            webview: None,
            error: None,
            bounds: None,
            visible: false,
            address,
            url: url.unwrap_or_else(|| "about:blank".into()),
            title: String::new(),
            loading: false,
            navigated,
            token: new_token(),
            logs: Logs::default(),
            to_view,
            queued: Vec::new(),
            _ipc: ipc,
            _subscriptions: subscriptions,
        }
    }

    /// The page as `app_state` shows it.
    pub fn describe(&self) -> Value {
        json!({ "url": self.current_url(), "title": self.title, "loading": self.loading })
    }

    fn current_url(&self) -> String {
        self.webview
            .as_ref()
            .and_then(|w| w.url().ok())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| self.url.clone())
    }

    fn navigate(&mut self, typed: &str, cx: &mut Context<Self>) {
        let url = normalize_address(typed);
        self.navigated = true;
        self.loading = true;
        self.url = url.clone();
        if let Some(webview) = &self.webview {
            let _ = webview.load_url(&url);
            if self.visible {
                let _ = webview.focus();
            }
        }
        cx.notify();
    }

    fn receive(&mut self, message: FromPage, window: &mut Window, cx: &mut Context<Self>) {
        match message {
            FromPage::Event(PageEvent::Console(entry)) => self.logs.push_console(entry),
            FromPage::Event(PageEvent::Network(entry)) => self.logs.push_network(entry),
            FromPage::Event(PageEvent::Url(url)) => {
                self.url = url;
                self.show_address(window, cx);
            }
            FromPage::Load { started, url } => {
                self.loading = started;
                if started && url != "about:blank" {
                    // Marks where each page's messages begin.
                    self.logs.push_console(ConsoleEntry {
                        level: "info".into(),
                        text: format!("— navigated to {url}"),
                        url: None,
                        line: None,
                        time_ms: 0.,
                    });
                }
                self.url = url;
                self.show_address(window, cx);
            }
            FromPage::Title(title) => self.title = title,
            FromPage::Open(url) => self.navigate(&url, cx),
            FromPage::Shoot { rect, reply } => match &self.webview {
                Some(webview) => snapshot(webview, rect, self.bounds, reply),
                None => {
                    let _ = reply.send(Err("The browser isn't open.".into()));
                }
            },
        }
    }

    /// Puts the page's address in the field, unless the user is typing there.
    fn show_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.address.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let shown = if self.url == "about:blank" {
            ""
        } else {
            self.url.as_str()
        };
        if self.address.read(cx).value().as_ref() != shown {
            let shown = shown.to_string();
            self.address
                .update(cx, |a, cx| a.set_value(shown, window, cx));
        }
    }

    /// Runs an agent's op; the receiver gets the answer.
    pub fn apply(&mut self, op: BrowserOp, cx: &mut Context<Self>) -> oneshot::Receiver<OpResult> {
        let (tx, rx) = oneshot::channel();
        if let Some(e) = &self.error {
            let _ = tx.send(Err(format!("The browser couldn't start: {e}")));
            return rx;
        }
        if self.webview.is_none() {
            self.queued.push((op, tx));
            return rx;
        }
        self.run(op, tx, cx);
        rx
    }

    fn run(&mut self, op: BrowserOp, reply: oneshot::Sender<OpResult>, cx: &mut Context<Self>) {
        let Some(webview) = self.webview.clone() else {
            let _ = reply.send(Err("The browser isn't open.".into()));
            return;
        };
        let value = match op {
            BrowserOp::Navigate { url } => {
                self.navigate(&url, cx);
                json!({ "navigating": self.url })
            }
            BrowserOp::Back | BrowserOp::Forward => {
                let back = matches!(op, BrowserOp::Back);
                let _ = webview.evaluate_script(if back {
                    "history.back()"
                } else {
                    "history.forward()"
                });
                json!({ "requested": if back { "back" } else { "forward" } })
            }
            BrowserOp::Reload => {
                self.loading = true;
                let _ = webview.reload();
                json!({ "reloading": self.current_url() })
            }
            BrowserOp::State => {
                let mut state = self.describe();
                state["console_errors"] = self
                    .logs
                    .console
                    .iter()
                    .filter(|e| e.level == "error")
                    .count()
                    .into();
                state["failed_requests"] = self
                    .logs
                    .network
                    .iter()
                    .filter(|e| e.failed())
                    .count()
                    .into();
                state
            }
            BrowserOp::Console {
                level,
                pattern,
                limit,
                clear,
            } => {
                let value = self
                    .logs
                    .console_json(level.as_deref(), pattern.as_deref(), limit);
                if clear {
                    self.logs.console.clear();
                }
                value
            }
            BrowserOp::Network {
                pattern,
                failed_only,
                limit,
                clear,
            } => {
                let value = self
                    .logs
                    .network_json(pattern.as_deref(), failed_only, limit);
                if clear {
                    self.logs.network.clear();
                }
                value
            }
            BrowserOp::Devtools => {
                webview.open_devtools();
                json!({ "opened": "Web Inspector" })
            }
            BrowserOp::Screenshot { target } if target.is_empty() => {
                return snapshot(&webview, None, self.bounds, reply);
            }
            BrowserOp::Screenshot { target } => {
                // The element's box first (scrolled into view), then the capture.
                let to_view = self.to_view.clone();
                let reply = Mutex::new(Some(reply));
                let script = agent_call(&rect_op(&target));
                let _ = webview.evaluate_script_with_callback(&script, move |result| {
                    let Some(reply) = reply.lock().ok().and_then(|mut r| r.take()) else {
                        return;
                    };
                    let rect = agent_result(&result).and_then(|r| {
                        let field = |k: &str| r[k].as_f64().unwrap_or_default();
                        let rect = (field("x"), field("y"), field("width"), field("height"));
                        if rect.2 < 1. || rect.3 < 1. {
                            return Err("The element isn't visible on screen.".to_string());
                        }
                        Ok(rect)
                    });
                    match rect {
                        Ok(rect) => {
                            let _ = to_view.unbounded_send(FromPage::Shoot {
                                rect: Some(rect),
                                reply,
                            });
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e));
                        }
                    }
                });
                return;
            }
            op => {
                let reply = Mutex::new(Some(reply));
                let script = agent_call(&serde_json::to_value(&op).unwrap_or_default());
                let sent = webview.evaluate_script_with_callback(&script, move |result| {
                    if let Some(reply) = reply.lock().ok().and_then(|mut r| r.take()) {
                        let _ = reply.send(agent_result(&result));
                    }
                });
                if let Err(e) = sent {
                    eprintln!("sawit: browser script failed: {e}");
                }
                return;
            }
        };
        let _ = reply.send(Ok(value));
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
            if !visible {
                let _ = webview.focus_parent();
            }
        }
    }

    fn place(&mut self, bounds: Bounds<Pixels>, window: &Window, cx: &mut Context<Self>) {
        if self.error.is_some() {
            return;
        }
        if self.webview.is_none() {
            match self.build(bounds, window) {
                Ok(webview) => {
                    self.webview = Some(Rc::new(webview));
                    for (op, reply) in std::mem::take(&mut self.queued) {
                        self.run(op, reply, cx);
                    }
                }
                Err(e) => {
                    self.error = Some(e.to_string().into());
                    for (_, reply) in self.queued.drain(..) {
                        let _ = reply.send(Err(format!("The browser couldn't start: {e}")));
                    }
                }
            }
            self.bounds = Some(bounds);
            return;
        }
        if self.bounds != Some(bounds) {
            self.bounds = Some(bounds);
            if let Some(webview) = &self.webview {
                let _ = webview.set_bounds(rect(bounds));
            }
        }
    }

    fn build(&self, bounds: Bounds<Pixels>, window: &Window) -> wry::Result<WebView> {
        let token = self.token.clone();
        let events = self.to_view.clone();
        let loads = self.to_view.clone();
        let titles = self.to_view.clone();
        let opens = self.to_view.clone();
        let mut builder = WebViewBuilder::new()
            .with_url(&self.url)
            .with_initialization_script_for_main_only(
                AGENT_JS.replace(TOKEN_PLACEHOLDER, &self.token),
                true,
            )
            .with_ipc_handler(move |request| {
                if let Some(event) = parse_ipc(request.body(), &token) {
                    let _ = events.unbounded_send(FromPage::Event(event));
                }
            })
            .with_on_page_load_handler(move |event, url| {
                let started = matches!(event, PageLoadEvent::Started);
                let _ = loads.unbounded_send(FromPage::Load { started, url });
            })
            .with_document_title_changed_handler(move |title| {
                let _ = titles.unbounded_send(FromPage::Title(title));
            })
            .with_navigation_handler(|url| {
                if is_web(&url) {
                    return true;
                }
                // mailto:, tel:, app links: the system's handler.
                let _ = std::process::Command::new("open").arg(&url).spawn();
                false
            })
            .with_new_window_req_handler(move |url, _| {
                let _ = opens.unbounded_send(FromPage::Open(url));
                NewWindowResponse::Deny
            })
            .with_devtools(true)
            .with_back_forward_navigation_gestures(true)
            .with_accept_first_mouse(true)
            .with_focused(self.visible)
            .with_visible(self.visible)
            .with_bounds(rect(bounds));
        if let Some(project) = &self.project {
            use wry::WebViewBuilderExtDarwin as _;
            builder =
                builder.with_data_store_identifier(sawit_core::browser::data_store_id(project));
        }
        builder.build_as_child(window)
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let loading = self.loading;
        h_flex()
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("browser-back")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ChevronLeft)
                    .tooltip("Back")
                    .on_click(cx.listener(|this, _, _, cx| {
                        drop(this.apply(BrowserOp::Back, cx));
                    })),
            )
            .child(
                Button::new("browser-forward")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ChevronRight)
                    .tooltip("Forward")
                    .on_click(cx.listener(|this, _, _, cx| {
                        drop(this.apply(BrowserOp::Forward, cx));
                    })),
            )
            .child(
                Button::new("browser-reload")
                    .ghost()
                    .xsmall()
                    .icon(if loading { Lucide::X } else { Lucide::RotateCw })
                    .tooltip(if loading { "Stop" } else { "Reload" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if loading {
                            if let Some(webview) = &this.webview {
                                let _ = webview.evaluate_script("window.stop()");
                            }
                            this.loading = false;
                            cx.notify();
                        } else {
                            drop(this.apply(BrowserOp::Reload, cx));
                        }
                    })),
            )
            .child(div().flex_1().child(Input::new(&self.address).small()))
            .child(
                Button::new("browser-inspect")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Code)
                    .tooltip("Web Inspector")
                    .on_click(cx.listener(|this, _, _, cx| {
                        drop(this.apply(BrowserOp::Devtools, cx));
                    })),
            )
    }
}

impl Drop for BrowserView {
    fn drop(&mut self) {
        if let Some(webview) = &self.webview {
            let _ = webview.focus_parent();
        }
    }
}

impl Render for BrowserView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(error) = &self.error {
            return Placeholder::error("The browser couldn't start")
                .detail(error.clone())
                .into_any_element();
        }
        let this = cx.entity().downgrade();
        v_flex()
            .size_full()
            .child(self.toolbar(cx))
            .child(
                div().flex_1().min_h_0().child(
                    canvas(
                        move |bounds, window, cx| {
                            let _ = this.update(cx, |this, cx| this.place(bounds, window, cx));
                        },
                        |_, _, _, _| {},
                    )
                    .size_full(),
                ),
            )
            .into_any_element()
    }
}

fn is_web(url: &str) -> bool {
    ["http:", "https:", "about:", "data:", "blob:", "file:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

fn new_token() -> String {
    use std::io::Read as _;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("read /dev/urandom");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Calls the page agent with `op`; an empty string means the page has no
/// agent (it is still loading, or isn't a web page).
fn agent_call(op: &Value) -> String {
    format!("(() => {{ const a = window.__sawitAgent; return a ? a.run({op}) : \"\"; }})()")
}

fn rect_op(target: &Target) -> Value {
    let mut op = serde_json::to_value(target).unwrap_or_else(|_| json!({}));
    op["op"] = "rect".into();
    op
}

/// The agent's answer as WebKit hands it over: its return value (a JSON
/// string) serialized as JSON once more.
fn agent_result(result: &str) -> OpResult {
    let text: String = serde_json::from_str(result).unwrap_or_default();
    if text.is_empty() {
        return Err(
            "The page isn't ready yet (it may still be loading, or isn't a web page). Try again after browser_wait_for."
                .into(),
        );
    }
    let mut value: Value =
        serde_json::from_str(&text).map_err(|e| format!("The page sent a bad answer: {e}"))?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("The page couldn't do that.")
            .to_string());
    }
    if let Some(map) = value.as_object_mut() {
        map.remove("ok");
    }
    Ok(value)
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

/// Captures the view (or `rect` of it, in points) as a PNG at 1×, answered
/// as `png_base64` with its size.
fn snapshot(
    webview: &WebView,
    rect: Option<(f64, f64, f64, f64)>,
    bounds: Option<Bounds<Pixels>>,
    reply: oneshot::Sender<OpResult>,
) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{NSDictionary, NSError, NSNumber};
    use objc2_web_kit::WKSnapshotConfiguration;
    use wry::WebViewExtMacOS as _;

    let Some(mtm) = MainThreadMarker::new() else {
        let _ = reply.send(Err("Screenshots are taken on the main thread.".into()));
        return;
    };
    let width = match rect {
        Some((_, _, w, _)) => w,
        None => bounds.map_or(1024., |b| f64::from(f32::from(b.size.width))),
    };
    let config = unsafe { WKSnapshotConfiguration::new(mtm) };
    unsafe {
        if let Some((x, y, w, h)) = rect {
            config.setRect(CGRect::new(CGPoint::new(x, y), CGSize::new(w, h)));
        }
        config.setSnapshotWidth(Some(&NSNumber::new_f64(width)));
        config.setAfterScreenUpdates(true);
    }
    let reply = Mutex::new(Some(reply));
    let handler = block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let Some(reply) = reply.lock().ok().and_then(|mut r| r.take()) else {
            return;
        };
        let result = (|| {
            if image.is_null() {
                let reason = unsafe { error.as_ref() }
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_else(|| "unknown error".into());
                return Err(format!("WebKit couldn't take the screenshot: {reason}"));
            }
            let image = unsafe { &*image };
            let tiff = image
                .TIFFRepresentation()
                .ok_or("The screenshot has no pixels.")?;
            let rep =
                NSBitmapImageRep::imageRepWithData(&tiff).ok_or("The screenshot has no bitmap.")?;
            let png = unsafe {
                rep.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .ok_or("The screenshot couldn't be saved as PNG.")?;
            let (png, w, h) = at_1x(png.to_vec(), width.round() as u32)?;
            Ok(json!({
                "png_base64": sawit_core::browser::base64_encode(&png),
                "width": w,
                "height": h,
            }))
        })();
        let _ = reply.send(result);
    });
    let wk = webview.webview();
    unsafe { wk.takeSnapshotWithConfiguration_completionHandler(Some(&config), &handler) };
}

/// Retina captures are twice the size in pixels; agents get them at the
/// page's CSS size, which is plenty and costs a quarter of the tokens.
fn at_1x(png: Vec<u8>, width: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|e| format!("The screenshot couldn't be read: {e}"))?;
    if width == 0 || image.width() <= width + width / 10 {
        let (w, h) = (image.width(), image.height());
        return Ok((png, w, h));
    }
    let height =
        (u64::from(image.height()) * u64::from(width) / u64::from(image.width())).max(1) as u32;
    let small = image.resize_exact(width, height, image::imageops::FilterType::Triangle);
    let mut out = std::io::Cursor::new(Vec::new());
    small
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| format!("The screenshot couldn't be saved: {e}"))?;
    Ok((out.into_inner(), width, height))
}

#[cfg(test)]
mod tests {
    use super::{Target, agent_call, agent_result, at_1x, is_web, new_token, rect_op};
    use serde_json::json;

    #[test]
    fn agent_answers() {
        let wire = serde_json::to_string(r#"{"ok":true,"n":1}"#).unwrap();
        assert_eq!(agent_result(&wire), Ok(json!({"n": 1})));
        let wire = serde_json::to_string(r#"{"ok":false,"error":"No element e9."}"#).unwrap();
        assert_eq!(agent_result(&wire), Err("No element e9.".into()));
        for nothing in ["", "\"\"", "null"] {
            assert!(agent_result(nothing).unwrap_err().contains("isn't ready"));
        }
    }

    #[test]
    fn agent_calls_embed_the_op() {
        let call = agent_call(&json!({"op": "click", "ref": "e2"}));
        assert!(call.contains(r#"a.run({"op":"click","ref":"e2"})"#));
        let op = rect_op(&Target {
            reference: Some("e5".into()),
            selector: None,
        });
        assert_eq!(op, json!({"ref": "e5", "op": "rect"}));
    }

    #[test]
    fn web_schemes_stay_in_the_tab() {
        assert!(is_web("https://a.dev") && is_web("about:blank") && is_web("file:///x"));
        assert!(!is_web("mailto:a@b.c") && !is_web("slack://open"));
    }

    #[test]
    fn tokens_are_random_hex() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn retina_screenshots_shrink_to_css_size() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(200, 100)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let (small, w, h) = at_1x(png.into_inner(), 100).unwrap();
        assert_eq!((w, h), (100, 50));
        assert_eq!(image::load_from_memory(&small).unwrap().width(), 100);
    }
}
