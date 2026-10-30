//! The browser tab: a WKWebView laid over the tab below a GPUI toolbar (back,
//! forward, reload, address, Web Inspector), placed and hidden like the
//! canvas's (see `canvas.rs`). Cookies and storage are kept per project.
//!
//! Agents drive it through [`BrowserView::apply`] (`sawit-mcp`'s `browser_*`
//! tools): navigation, state, logs and screenshots are answered here; the
//! rest by the page agent (`sawit_core::browser::AGENT_JS`), injected into
//! every page, through `Engine::eval_with_callback`.

mod engine;

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

use engine::{Engine, EngineEvent, EngineSpec, Frame};

use crate::rt;
use crate::states::Placeholder;

/// The tab's icon and title.
pub const ICON: Lucide = Lucide::Globe;
pub const TITLE: &str = "Browser";

const TOOLBAR_HEIGHT: f32 = 36.;

type OpResult = Result<Value, String>;

/// What the engine reports, from its handlers (any thread) to the view.
enum FromPage {
    Event(PageEvent),
    Engine(EngineEvent),
    /// An element's box is known: capture it (or the whole view).
    Shoot {
        rect: Option<(f64, f64, f64, f64)>,
        reply: oneshot::Sender<OpResult>,
    },
}

pub struct BrowserView {
    project: Option<PathBuf>,
    engine: Option<Rc<Engine>>,
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
                    if let Some(engine) = &this.engine {
                        engine.focus_parent();
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
            engine: None,
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
        self.engine
            .as_ref()
            .and_then(|e| e.url())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| self.url.clone())
    }

    fn navigate(&mut self, typed: &str, cx: &mut Context<Self>) {
        let url = normalize_address(typed);
        self.navigated = true;
        self.loading = true;
        self.url = url.clone();
        if let Some(engine) = &self.engine {
            engine.load_url(&url);
            if self.visible {
                engine.focus();
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
            FromPage::Engine(EngineEvent::Ipc(body)) => {
                if let Some(event) = parse_ipc(&body, &self.token) {
                    self.receive(FromPage::Event(event), window, cx);
                }
            }
            FromPage::Engine(EngineEvent::Load { started, url }) => {
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
            FromPage::Engine(EngineEvent::Title(title)) => self.title = title,
            FromPage::Engine(EngineEvent::NewWindow(url)) => self.navigate(&url, cx),
            FromPage::Shoot { rect, reply } => match &self.engine {
                Some(engine) => {
                    let width = shot_width(rect, self.bounds);
                    engine.snapshot(rect, width, 1., reply)
                }
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
        if self.engine.is_none() {
            self.queued.push((op, tx));
            return rx;
        }
        self.run(op, tx, cx);
        rx
    }

    fn run(&mut self, op: BrowserOp, reply: oneshot::Sender<OpResult>, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
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
                engine.eval(if back {
                    "history.back()"
                } else {
                    "history.forward()"
                });
                json!({ "requested": if back { "back" } else { "forward" } })
            }
            BrowserOp::Reload => {
                self.loading = true;
                engine.reload();
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
                engine.open_devtools();
                json!({ "opened": "Web Inspector" })
            }
            BrowserOp::Screenshot { target } if target.is_empty() => {
                let width = shot_width(None, self.bounds);
                return engine.snapshot(None, width, 1., reply);
            }
            BrowserOp::Screenshot { target } => {
                // The element's box first (scrolled into view), then the capture.
                let to_view = self.to_view.clone();
                let reply = Mutex::new(Some(reply));
                let script = agent_call(&rect_op(&target));
                let _ = engine.eval_with_callback(&script, move |result| {
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
                let sent = engine.eval_with_callback(&script, move |result| {
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
        if let Some(engine) = &self.engine {
            engine.set_visible(visible);
            if !visible {
                engine.focus_parent();
            }
        }
    }

    fn place(&mut self, bounds: Bounds<Pixels>, window: &Window, cx: &mut Context<Self>) {
        if self.error.is_some() {
            return;
        }
        if self.engine.is_none() {
            match self.build(bounds, window) {
                Ok(engine) => {
                    self.engine = Some(Rc::new(engine));
                    for (op, reply) in std::mem::take(&mut self.queued) {
                        self.run(op, reply, cx);
                    }
                }
                Err(e) => {
                    self.error = Some(e.clone().into());
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
            if let Some(engine) = &self.engine {
                engine.set_frame(frame(bounds));
            }
        }
    }

    fn build(&self, bounds: Bounds<Pixels>, window: &Window) -> Result<Engine, String> {
        let to_view = self.to_view.clone();
        Engine::build(
            EngineSpec {
                url: self.url.clone(),
                init_script: AGENT_JS.replace(TOKEN_PLACEHOLDER, &self.token),
                data_store: self
                    .project
                    .as_deref()
                    .map(sawit_core::browser::data_store_id),
                visible: self.visible,
                frame: frame(bounds),
            },
            move |event| {
                let _ = to_view.unbounded_send(FromPage::Engine(event));
            },
            window,
        )
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
                            if let Some(engine) = &this.engine {
                                engine.eval("window.stop()");
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
        if let Some(engine) = &self.engine {
            engine.focus_parent();
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

fn frame(bounds: Bounds<Pixels>) -> Frame {
    Frame {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
    }
}

/// The width a capture is scaled to: the element's, else the view's.
fn shot_width(rect: Option<(f64, f64, f64, f64)>, bounds: Option<Bounds<Pixels>>) -> f64 {
    match rect {
        Some((_, _, w, _)) => w,
        None => bounds.map_or(1024., |b| f64::from(f32::from(b.size.width))),
    }
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

#[cfg(test)]
mod tests {
    use super::{Target, agent_call, agent_result, new_token, rect_op};
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
    fn tokens_are_random_hex() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
