//! The browser tab: a WKWebView laid over the tab below a GPUI toolbar (back,
//! forward, reload, address, Web Inspector), placed and hidden like the
//! canvas's (see `canvas.rs`). Cookies and storage are kept per project.
//!
//! Agents drive it through [`BrowserView::apply`] (`sawit-mcp`'s `browser_*`
//! tools): navigation, state, logs and screenshots are answered here; the
//! rest by the page agent (`sawit_core::browser::page_script`), injected into
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
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::browser::{
    BrowserOp, ConsoleEntry, Logs, PageEvent, Target, ViewportMode, fit, normalize_address,
    page_script, parse_ipc, resize_mode,
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
/// Around the page in device mode, and the strip above it for the caption.
const DEVICE_PADDING: f32 = 16.;
const CAPTION_BAND: f32 = 24.;

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
    /// Selector of the element picked last.
    last_pick: Option<String>,
    /// Responsive, or a device size (kept per tab).
    viewport: ViewportMode,
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
            last_pick: None,
            viewport: ViewportMode::Responsive,
            to_view,
            queued: Vec::new(),
            _ipc: ipc,
            _subscriptions: subscriptions,
        }
    }

    /// The page as `app_state` shows it.
    pub fn describe(&self) -> Value {
        json!({
            "url": self.current_url(),
            "title": self.title,
            "loading": self.loading,
            "viewport": self.viewport_json(),
        })
    }

    /// The CSS viewport the page sees, and the zoom that makes it fit.
    fn viewport_json(&self) -> Value {
        let (width, height, device, scale) = match (&self.viewport, self.bounds) {
            (
                ViewportMode::Fixed {
                    name,
                    width,
                    height,
                },
                Some(bounds),
            ) => {
                let (_, scale) = page_frame(frame(bounds), &self.viewport);
                (*width as f64, *height as f64, name.clone(), scale)
            }
            (
                ViewportMode::Fixed {
                    name,
                    width,
                    height,
                },
                None,
            ) => (*width as f64, *height as f64, name.clone(), 1.),
            (ViewportMode::Responsive, bounds) => {
                let (w, h) = bounds.map_or((0., 0.), |b| {
                    (
                        f64::from(f32::from(b.size.width)).round(),
                        f64::from(f32::from(b.size.height)).round(),
                    )
                });
                (w, h, Some("responsive".to_string()), 1.)
            }
        };
        json!({ "width": width, "height": height, "device": device, "scale": scale })
    }

    /// Puts the web view where the viewport mode wants it, with the zoom
    /// that keeps the CSS viewport at the device size.
    fn apply_layout(&self) {
        if let (Some(engine), Some(bounds)) = (&self.engine, self.bounds) {
            let (frame, scale) = page_frame(frame(bounds), &self.viewport);
            engine.set_frame(frame);
            engine.set_zoom(scale);
        }
    }

    /// The page zoom in force (1 unless a device is scaled down).
    fn scale(&self) -> f64 {
        self.bounds
            .map_or(1., |b| page_frame(frame(b), &self.viewport).1)
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
            FromPage::Event(PageEvent::Picked(pick)) => {
                self.last_pick = pick["element"]["selector"].as_str().map(str::to_string);
                cx.notify();
            }
            // Hovers wait for the design panel; keys are handled with the panel.
            FromPage::Event(PageEvent::Hovered(_) | PageEvent::Key(_)) => {}
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
                    let width = shot_width(rect, self.bounds, &self.viewport);
                    engine.snapshot(rect, width, self.scale(), reply)
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
            BrowserOp::Resize {
                device,
                width,
                height,
            } => match resize_mode(device.as_deref(), width, height) {
                Ok(mode) => {
                    self.viewport = mode;
                    self.apply_layout();
                    cx.notify();
                    self.viewport_json()
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                    return;
                }
            },
            BrowserOp::Screenshot { target } if target.is_empty() => {
                let width = shot_width(None, self.bounds, &self.viewport);
                return engine.snapshot(None, width, self.scale(), reply);
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
                    self.bounds = Some(bounds);
                    self.apply_layout();
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
            self.apply_layout();
        }
    }

    fn build(&self, bounds: Bounds<Pixels>, window: &Window) -> Result<Engine, String> {
        let to_view = self.to_view.clone();
        Engine::build(
            EngineSpec {
                url: self.url.clone(),
                init_script: page_script(&self.token),
                data_store: self
                    .project
                    .as_deref()
                    .map(sawit_core::browser::data_store_id),
                visible: self.visible,
                frame: page_frame(frame(bounds), &self.viewport).0,
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
        let device = match &self.viewport {
            ViewportMode::Fixed {
                name,
                width,
                height,
            } => Some(caption(name.as_deref(), *width, *height, self.scale())),
            ViewportMode::Responsive => None,
        };
        v_flex()
            .size_full()
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .when(device.is_some(), |d| d.bg(cx.theme().muted))
                    .children(device.map(|text| {
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(CAPTION_BAND))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(text)
                    }))
                    .child(
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

/// Where the web view goes in `bounds`, and the page zoom: the page itself
/// in responsive mode, else the device size scaled to fit and centered
/// below the caption band.
fn page_frame(bounds: Frame, mode: &ViewportMode) -> (Frame, f64) {
    let ViewportMode::Fixed { width, height, .. } = mode else {
        return (bounds, 1.);
    };
    let area = (
        (bounds.width - 2. * DEVICE_PADDING).max(1.),
        (bounds.height - 2. * DEVICE_PADDING - CAPTION_BAND).max(1.),
    );
    let fit = fit((*width, *height), area);
    let frame = Frame {
        x: bounds.x + DEVICE_PADDING + (area.0 - fit.width) / 2.,
        y: bounds.y + CAPTION_BAND + DEVICE_PADDING + (area.1 - fit.height) / 2.,
        width: fit.width,
        height: fit.height,
    };
    (frame, fit.scale)
}

/// `iPhone 15 · 393 × 852 · 75%`: no name when there is none, no percentage
/// at full size.
fn caption(name: Option<&str>, width: u32, height: u32, scale: f64) -> String {
    let mut parts: Vec<String> = name.map(str::to_string).into_iter().collect();
    parts.push(format!("{width} × {height}"));
    let percent = (scale * 100.).round() as u32;
    if percent < 100 {
        parts.push(format!("{percent}%"));
    }
    parts.join(" · ")
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
fn shot_width(
    rect: Option<(f64, f64, f64, f64)>,
    bounds: Option<Bounds<Pixels>>,
    mode: &ViewportMode,
) -> f64 {
    match (rect, mode) {
        (Some((_, _, w, _)), _) => w,
        (None, ViewportMode::Fixed { width, .. }) => f64::from(*width),
        (None, ViewportMode::Responsive) => {
            bounds.map_or(1024., |b| f64::from(f32::from(b.size.width)))
        }
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
    use super::{
        Frame, Target, ViewportMode, agent_call, agent_result, caption, new_token, page_frame,
        rect_op,
    };
    use serde_json::json;

    #[test]
    fn devices_are_centered_below_the_caption() {
        let bounds = Frame {
            x: 0.,
            y: 0.,
            width: 800.,
            height: 1000.,
        };
        let phone = ViewportMode::Fixed {
            name: Some("iPhone 15".into()),
            width: 393,
            height: 852,
        };
        assert_eq!(
            page_frame(bounds, &phone),
            (
                Frame {
                    x: 203.5,
                    y: 86.,
                    width: 393.,
                    height: 852.
                },
                1.0
            )
        );
        assert_eq!(page_frame(bounds, &ViewportMode::Responsive), (bounds, 1.0));
        // Scaled to fit a small area; offset by the bounds' origin.
        let small = Frame {
            x: 10.,
            y: 20.,
            width: 232.,
            height: 440.,
        };
        let (f, scale) = page_frame(small, &phone);
        assert!((scale - 384. / 852.).abs() < 1e-6 && f.x >= 26. && (f.y - 60.).abs() < 1e-3);
    }

    #[test]
    fn captions() {
        assert_eq!(
            caption(Some("iPhone 15"), 393, 852, 0.75),
            "iPhone 15 · 393 × 852 · 75%"
        );
        assert_eq!(caption(None, 800, 600, 1.), "800 × 600");
    }

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
