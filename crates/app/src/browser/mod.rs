//! The browser tab: a WKWebView laid over the tab below a GPUI toolbar (back,
//! forward, reload, address, Web Inspector), placed and hidden like the
//! canvas's (see `canvas.rs`). Cookies and storage are kept per project.
//!
//! Agents drive it through [`BrowserView::apply`] (`sawit-mcp`'s `browser_*`
//! tools): navigation, state, logs and screenshots are answered here; the
//! rest by the page agent (`sawit_core::browser::page_script`), injected into
//! every page, through `Engine::eval_with_callback`.

mod annotate;
mod design;
mod engine;
mod panel;
mod toolbar;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::annotations::{
    Annotation, ElementDetails, Intent, PageGroup, PageInfo, next_number, page_key,
    relative_source, screenshot_dir, screenshot_name,
};
use sawit_core::browser::{
    BrowserOp, ConsoleEntry, Logs, PageEvent, Target, ViewportMode, fit, normalize_address,
    page_script, parse_ipc, resize_mode,
};
use sawit_core::probe;
use serde_json::{Value, json};

use annotate::{AnnotateEvent, AnnotatePanel, group_order, padded_rect};
use design::{DesignEvent, DesignPanel};
use engine::{Engine, EngineEvent, EngineSpec, Frame};

use crate::rt;
use crate::states::Placeholder;

/// The tab's icon and title.
pub const ICON: Lucide = Lucide::Globe;
pub const TITLE: &str = "Browser";

actions!(browser, [ToggleAnnotate]);

/// ⌘⇧. toggles Annotate while Sawit has the keyboard; with the page focused
/// the overlay posts a `key` message instead.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "cmd-shift-.",
        ToggleAnnotate,
        Some("Browser"),
    )]);
}

/// The side panel's modes; one at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Annotate,
    Design,
}

impl Panel {
    fn as_str(self) -> &'static str {
        match self {
            Panel::Annotate => "annotate",
            Panel::Design => "design",
        }
    }

    /// What picking does while this panel is open.
    fn pick_mode(self) -> PickMode {
        match self {
            Panel::Annotate => PickMode::Annotate,
            Panel::Design => PickMode::Inspect,
        }
    }
}

/// What a click on the page does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickMode {
    Off,
    Annotate,
    Inspect,
}

impl PickMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PickMode::Off => "off",
            PickMode::Annotate => "annotate",
            PickMode::Inspect => "inspect",
        }
    }
}

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

/// What the browser asks of the workspace.
pub enum BrowserEvent {
    /// Paste this markdown into the terminal running an agent.
    SendToTerminal(String),
}

impl EventEmitter<BrowserEvent> for BrowserView {}

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
    /// Keeps the "Browser" key context on the dispatch path (⌘⇧.).
    focus: FocusHandle,
    panel: Option<Panel>,
    annotate: Entity<AnnotatePanel>,
    design_panel: Entity<DesignPanel>,
    /// Shots take turns: the overlay has one hidden flag.
    shots: ShotGate,
    pick: PickMode,
    /// Picking was asked for but the page had no agent yet; it is armed
    /// again when the page has loaded.
    pick_waiting: bool,
    to_view: UnboundedSender<FromPage>,
    /// The design scan (cached per page), inspect and export.
    design: design::flow::DesignFlow,
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
        let annotate = cx.new(|cx| AnnotatePanel::new(window, cx));
        let design_panel = cx.new(|_| DesignPanel::new());
        let subscriptions = vec![
            cx.subscribe_in(
                &annotate,
                window,
                |this, _, event: &AnnotateEvent, _, cx| this.on_annotate(event, cx),
            ),
            cx.subscribe_in(
                &design_panel,
                window,
                |this, _, event: &DesignEvent, _, cx| this.on_design(event, cx),
            ),
            cx.subscribe_in(
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
            ),
        ];
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
            focus: cx.focus_handle(),
            panel: None,
            annotate,
            design_panel,
            shots: ShotGate::default(),
            pick: PickMode::Off,
            pick_waiting: false,
            to_view,
            design: Default::default(),
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
            "panel": self.panel.map(Panel::as_str),
            "pick_mode": self.pick.as_str(),
            "last_pick": self.last_pick,
        })
    }

    /// Gives the keyboard to the view (not the page), so ⌘⇧. reaches it.
    fn take_keyboard(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(engine) = &self.engine {
            engine.focus_parent();
        }
        self.focus.focus(window, cx);
    }

    pub fn panel(&self) -> Option<Panel> {
        self.panel
    }

    /// Opens `panel`, or closes it when it is the one open. Annotate arms
    /// element picking, Design inspecting; closing disarms.
    pub fn toggle_panel(&mut self, panel: Panel, _window: &mut Window, cx: &mut Context<Self>) {
        if self.panel == Some(panel) {
            self.panel = None;
            self.set_pick(PickMode::Off, cx);
        } else {
            self.panel = Some(panel);
            self.set_pick(panel.pick_mode(), cx);
            self.scan_if_design_open(cx);
        }
        cx.notify();
    }

    /// ⌘⇧.: like the Annotate button, except that with Annotate open after Esc
    /// it picks again instead of closing.
    fn annotate_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel == Some(Panel::Annotate) && self.pick == PickMode::Off {
            self.set_pick(PickMode::Annotate, cx);
        } else {
            self.toggle_panel(Panel::Annotate, window, cx);
        }
    }

    /// Arms (or disarms) picking on the page.
    fn set_pick(&mut self, mode: PickMode, cx: &mut Context<Self>) {
        self.pick = mode;
        if mode == PickMode::Off {
            self.pick_waiting = false;
        }
        self.send_pick(pick_op(mode), cx);
        cx.notify();
    }

    /// Sends a pick op; when the page has no agent yet, remembers to try
    /// again once it has loaded.
    fn send_pick(&mut self, op: Value, cx: &mut Context<Self>) {
        let armed = self.pick;
        let reply = self.page_op(op);
        cx.spawn(async move |this, cx| {
            let failed = !matches!(reply.await, Ok(Ok(_)));
            let _ = this.update(cx, |this, cx| {
                if this.pick == armed {
                    this.pick_waiting = failed && armed != PickMode::Off;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Ops to run when a page has loaded: its agent starts fresh, so
    /// picking has to be armed again.
    fn ops_after_load(&self, cx: &App) -> Vec<Value> {
        let mut ops = rearm_ops(self.pick);
        let markers = self.annotate.read(cx).markers_for(&self.current_url());
        if !markers.is_empty() {
            ops.push(markers_op(&markers));
        }
        ops
    }

    /// Draws the numbered markers of the page on show (none clears them).
    fn sync_markers(&self, cx: &App) {
        let markers = self.annotate.read(cx).markers_for(&self.current_url());
        drop(self.page_op(markers_op(&markers)));
    }

    /// Shows `text` in the Annotate footer for a moment.
    pub fn set_annotate_note(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.annotate.update(cx, |a, cx| a.set_note(text, cx));
    }

    fn on_design(&mut self, event: &DesignEvent, cx: &mut Context<Self>) {
        match event {
            DesignEvent::Flash(selector) => {
                drop(self.page_op(json!({ "op": "flash", "selector": selector })));
            }
            DesignEvent::CopyHtml(selector) => {
                // Not the latest-only path: a hover meanwhile neither drops nor is dropped.
                let info = self.style_info(json!({ "selector": selector }), true, cx);
                cx.spawn(async move |this, cx| {
                    let note = match info.await {
                        Ok(info) => match info.html {
                            Some(html) => {
                                cx.update(|cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(html));
                                });
                                "Copied".to_string()
                            }
                            None => "The page gave no markup.".to_string(),
                        },
                        Err(e) => e,
                    };
                    let _ = this.update(cx, |this, cx| this.design_note(note, cx));
                })
                .detach();
            }
            DesignEvent::SaveAssets(assets) => self.save_assets(assets.clone(), cx),
            DesignEvent::SaveAll { include_videos } => self.save_all_assets(*include_videos, cx),
            DesignEvent::Export { include_videos } => self.export_from_panel(*include_videos, cx),
            DesignEvent::Reveal(file) => {
                let file = file.clone();
                let job = crate::rt::spawn_blocking(cx, move || {
                    let status = std::process::Command::new("open")
                        .arg("-R")
                        .arg(&file)
                        .status()
                        .map_err(|e| e.to_string())?;
                    status
                        .success()
                        .then_some(())
                        .ok_or_else(|| format!("open exited with {status}"))
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Err(e)) = job.await {
                        let _ = this.update(cx, |this, cx| {
                            this.design_note(format!("Couldn't reveal in Finder: {e}"), cx)
                        });
                    }
                })
                .detach();
            }
        }
    }

    fn on_annotate(&mut self, event: &AnnotateEvent, cx: &mut Context<Self>) {
        match event {
            AnnotateEvent::Show(selector) => {
                drop(self.page_op(json!({ "op": "flash", "selector": selector })));
            }
            AnnotateEvent::Changed => self.sync_markers(cx),
            AnnotateEvent::Copy => {
                if let Some(md) = self.annotations_markdown(cx) {
                    cx.write_to_clipboard(ClipboardItem::new_string(md));
                    self.annotate.update(cx, |a, cx| a.set_note("Copied", cx));
                }
            }
            AnnotateEvent::Send => {
                if let Some(md) = self.annotations_markdown(cx) {
                    cx.emit(BrowserEvent::SendToTerminal(md));
                }
            }
        }
    }

    /// The annotations as the markdown agents read; `None` without any.
    pub fn annotations_markdown(&self, cx: &App) -> Option<String> {
        let panel = self.annotate.read(cx);
        let items = panel.annotations(cx);
        if items.is_empty() {
            return None;
        }
        let url = self.current_url();
        let viewport = self.viewport_json();
        let size = (
            viewport["width"].as_f64().unwrap_or_default() as u32,
            viewport["height"].as_f64().unwrap_or_default() as u32,
        );
        let device = match &self.viewport {
            ViewportMode::Fixed { name, .. } => name.clone(),
            ViewportMode::Responsive => None,
        };
        let groups: Vec<PageGroup> = group_order(&url, &items)
            .into_iter()
            .map(|key| {
                let title = if key == page_key(&url) && !self.title.is_empty() {
                    self.title.clone()
                } else {
                    key.clone()
                };
                PageGroup {
                    page: PageInfo {
                        title,
                        url: key.clone(),
                        viewport: size,
                        device: device.clone(),
                    },
                    items: items
                        .iter()
                        .filter(|a| page_key(&a.url) == key)
                        .cloned()
                        .collect(),
                }
            })
            .collect();
        let errors = if panel.include_errors() {
            self.logs.page_errors(20)
        } else {
            Vec::new()
        };
        Some(sawit_core::annotations::markdown(
            &groups,
            Some((page_key(&url), &errors)),
        ))
    }

    /// An element was clicked in annotate mode: capture it, then add a row
    /// (whose comment gets the keyboard) and redraw the markers.
    fn on_picked(
        &mut self,
        mut element: ElementDetails,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = self.current_url();
        if let Some(source) = &mut element.source {
            source.file = relative_source(&source.file, self.project.as_deref());
        }
        let viewport = self.viewport_json();
        let size = (
            viewport["width"].as_f64().unwrap_or_default() as u32,
            viewport["height"].as_f64().unwrap_or_default() as u32,
        );
        let rect = padded_rect(&element.rect, size, 8.);
        let (tx, rx) = oneshot::channel();
        if rect.2 >= 1. && rect.3 >= 1. {
            self.shoot(Some(rect), tx, cx);
        } else {
            let _ = tx.send(Err("The element isn't on screen.".into()));
        }
        let project = self.project.clone();
        cx.spawn_in(window, async move |this, cx| {
            let shot = rx
                .await
                .unwrap_or_else(|_| Err("The screenshot was cancelled.".into()));
            let _ = this.update_in(cx, |this, window, cx| {
                let existing = this.annotate.read(cx).annotations(cx);
                let n = next_number(&existing, &url);
                let screenshot =
                    match shot.and_then(|v| save_screenshot(&v, project.as_deref(), &url, n)) {
                        Ok(path) => Some(path),
                        Err(e) => {
                            eprintln!("sawit: no screenshot for annotation {n}: {e}");
                            None
                        }
                    };
                let annotation = Annotation {
                    n,
                    url,
                    element,
                    comment: String::new(),
                    intent: Intent::Fix,
                    screenshot,
                    include_screenshot: true,
                };
                // The keyboard moves from the page to the panel.
                if let Some(engine) = &this.engine {
                    engine.focus_parent();
                }
                this.annotate
                    .update(cx, |panel, cx| panel.add(annotation, window, cx));
            });
        })
        .detach();
    }

    /// Runs `op` on the page and answers with what the page says.
    pub(crate) fn page_op(&self, op: Value) -> oneshot::Receiver<OpResult> {
        let (tx, rx) = oneshot::channel();
        let Some(engine) = &self.engine else {
            let _ = tx.send(Err("The browser isn't open.".into()));
            return rx;
        };
        let tx = Arc::new(Mutex::new(Some(tx)));
        let callback_tx = tx.clone();
        let sent = engine.eval_with_callback(&agent_call(&op), move |result| {
            if let Some(tx) = callback_tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(agent_result(&result));
            }
        });
        if let Err(e) = sent
            && let Some(tx) = tx.lock().ok().and_then(|mut t| t.take())
        {
            let _ = tx.send(Err(format!("The page couldn't be reached: {e}")));
        }
        rx
    }

    /// Takes a screenshot of the page without the overlay in it: hides the
    /// overlay, captures, and shows the overlay again whatever happened.
    pub(crate) fn shoot(
        &mut self,
        css_rect: Option<(f64, f64, f64, f64)>,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        let turn = self.shots.enter();
        cx.spawn(async move |this, cx| {
            // Another shot has the overlay hidden: wait for it to finish.
            if let Some(turn) = turn {
                let _ = turn.await;
            }
            let [hide, show] = shoot_ops();
            let hidden = this.update(cx, |this, _| this.page_op(hide));
            // A page without an agent has no overlay to hide.
            if let Ok(hidden) = hidden {
                let _ = hidden.await;
            }
            let (tx, rx) = oneshot::channel();
            let started = this.update(cx, |this, _| this.snap(css_rect, tx));
            let result = if started.is_ok() {
                rx.await
                    .unwrap_or_else(|_| Err("The screenshot was cancelled.".into()))
            } else {
                Err("The browser isn't open.".into())
            };
            let _ = this.update(cx, |this, _| {
                drop(this.page_op(show));
                this.shots.leave();
            });
            let _ = reply.send(result);
        })
        .detach();
    }

    /// Captures the view, or `rect` of the page.
    fn snap(&self, rect: Option<(f64, f64, f64, f64)>, reply: oneshot::Sender<OpResult>) {
        match &self.engine {
            Some(engine) => {
                let width = shot_width(rect, self.bounds, &self.viewport);
                engine.snapshot(rect, width, self.scale(), reply)
            }
            None => {
                let _ = reply.send(Err("The browser isn't open.".into()));
            }
        }
    }

    /// Sets how the page is sized in the tab.
    fn set_viewport(&mut self, mode: ViewportMode, cx: &mut Context<Self>) {
        self.viewport = mode;
        self.apply_layout();
        cx.notify();
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
                if pick["mode"] == "annotate" {
                    match serde_json::from_value(pick["element"].clone()) {
                        Ok(element) => self.on_picked(element, window, cx),
                        Err(e) => eprintln!("sawit: a picked element couldn't be read: {e}"),
                    }
                } else if let Some(selector) = &self.last_pick {
                    self.inspect_element(selector.clone(), true, cx);
                }
                cx.notify();
            }
            FromPage::Event(PageEvent::Hovered(hover)) => {
                if let Some(selector) = hover["selector"].as_str() {
                    self.inspect_element(selector.to_string(), false, cx);
                }
            }
            // The page has the keyboard: it forwards ⌘⇧. and Esc.
            FromPage::Event(PageEvent::Key(key)) => match key.as_str() {
                "toggle-annotate" => self.annotate_key(window, cx),
                "escape" => self.set_pick(PickMode::Off, cx),
                _ => {}
            },
            FromPage::Event(PageEvent::Url(url)) => {
                self.url = url;
                self.design_url_changed();
                self.scan_if_design_open(cx);
                self.show_address(window, cx);
                // Route changes without a load still move to another page.
                let url = self.current_url();
                self.annotate.update(cx, |a, cx| a.set_current(&url, cx));
                self.sync_markers(cx);
            }
            FromPage::Engine(EngineEvent::Ipc(body)) => {
                if let Some(event) = parse_ipc(&body, &self.token) {
                    self.receive(FromPage::Event(event), window, cx);
                }
            }
            FromPage::Engine(EngineEvent::Load { started, url }) => {
                self.loading = started;
                if started {
                    self.design_load_started();
                    self.sync_design(cx);
                }
                if !started {
                    self.scan_if_design_open(cx);
                    for op in self.ops_after_load(cx) {
                        if op["op"] == "pick" {
                            self.send_pick(op, cx);
                        } else {
                            drop(self.page_op(op));
                        }
                    }
                }
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
                let url = self.current_url();
                self.annotate.update(cx, |a, cx| a.set_current(&url, cx));
            }
            FromPage::Engine(EngineEvent::Title(title)) => self.title = title,
            FromPage::Engine(EngineEvent::NewWindow(url)) => self.navigate(&url, cx),
            FromPage::Shoot { rect, reply } => self.shoot(rect, reply, cx),
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
                    self.set_viewport(mode, cx);
                    self.viewport_json()
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                    return;
                }
            },
            BrowserOp::Annotations { clear } => {
                let markdown = self.annotations_markdown(cx);
                let count = self.annotate.read(cx).annotations(cx).len();
                if clear {
                    self.annotate.update(cx, |a, cx| a.clear(cx));
                }
                json!({ "markdown": markdown, "count": count })
            }
            BrowserOp::Inspect { target, format } => {
                let target = serde_json::to_value(&target).unwrap_or_default();
                return self.inspect_op(target, format, reply, cx);
            }
            BrowserOp::Design { part } => return self.design_op(part, reply, cx),
            BrowserOp::ExportDesign { include_videos } => {
                let _ = reply.send(self.export_op(include_videos, cx));
                return;
            }
            BrowserOp::ExportStatus => {
                let _ = reply.send(self.export_status_op());
                return;
            }
            BrowserOp::Screenshot { target } if target.is_empty() => {
                return self.shoot(None, reply, cx);
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
}

impl Drop for BrowserView {
    fn drop(&mut self) {
        if let Some(engine) = &self.engine {
            engine.focus_parent();
        }
    }
}

impl Focusable for BrowserView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
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
        let page = div()
            .flex_1()
            .min_w_0()
            .h_full()
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
            );
        let side = self.panel.map(|panel| self.side_panel(panel, cx));
        v_flex()
            .key_context("Browser")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleAnnotate, window, cx| {
                this.annotate_key(window, cx);
            }))
            .size_full()
            .child(self.toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(page)
                    .children(side),
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
    // Whole points, as `fit` gives the size, so WebKit doesn't round the frame.
    let frame = Frame {
        x: (bounds.x + DEVICE_PADDING + (area.0 - fit.width) / 2.).round(),
        y: (bounds.y + CAPTION_BAND + DEVICE_PADDING + (area.1 - fit.height) / 2.).round(),
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

fn overlay_op(hidden: bool) -> Value {
    json!({ "op": "overlay", "hidden": hidden })
}

fn pick_op(mode: PickMode) -> Value {
    json!({ "op": "pick", "mode": mode.as_str() })
}

/// What a page needs after it loads: its picker armed again.
fn rearm_ops(pick: PickMode) -> Vec<Value> {
    match pick {
        PickMode::Off => vec![],
        mode => vec![pick_op(mode)],
    }
}

/// Numbers drawn over the annotated elements of the page.
fn markers_op(markers: &[(u32, String)]) -> Value {
    let items: Vec<Value> = markers
        .iter()
        .map(|(n, selector)| json!({ "n": n, "selector": selector }))
        .collect();
    json!({ "op": "markers", "items": items })
}

/// Writes a screenshot answer (`png_base64`) to the project's annotation
/// folder (else the app cache); `None` when it can't be saved.
fn save_screenshot(
    shot: &Value,
    project: Option<&std::path::Path>,
    url: &str,
    n: u32,
) -> Result<PathBuf, String> {
    let png = shot["png_base64"]
        .as_str()
        .and_then(sawit_core::browser::base64_decode)
        .ok_or("the capture has no readable image")?;
    let cache = sawit_core::instances::AppPaths::default_macos().cache_dir();
    let dir = screenshot_dir(project, &cache, &sawit_core::date::local_date());
    match project {
        Some(project) => sawit_core::nyawit::ensure(project, &dir),
        None => std::fs::create_dir_all(&dir),
    }
    .map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = sawit_core::nyawit::unique(&dir, &screenshot_name(url, n));
    std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Lets one screenshot at a time hide the overlay: `enter` returns `None`
/// when it is free, else a receiver that fires on this shot's turn; every
/// shot calls `leave` when done.
#[derive(Default)]
struct ShotGate {
    busy: bool,
    waiting: std::collections::VecDeque<oneshot::Sender<()>>,
}

impl ShotGate {
    fn enter(&mut self) -> Option<oneshot::Receiver<()>> {
        if !self.busy {
            self.busy = true;
            return None;
        }
        let (tx, rx) = oneshot::channel();
        self.waiting.push_back(tx);
        Some(rx)
    }

    fn leave(&mut self) {
        // A waiter that went away is skipped.
        while let Some(next) = self.waiting.pop_front() {
            if next.send(()).is_ok() {
                return;
            }
        }
        self.busy = false;
    }
}

/// Around every screenshot: hide the overlay, then show it again.
fn shoot_ops() -> [Value; 2] {
    [overlay_op(true), overlay_op(false)]
}

/// The same size with width and height swapped; nothing in responsive mode.
fn rotated(mode: &ViewportMode) -> Option<ViewportMode> {
    match mode {
        ViewportMode::Responsive => None,
        ViewportMode::Fixed {
            name,
            width,
            height,
        } => Some(ViewportMode::Fixed {
            name: name.clone(),
            width: *height,
            height: *width,
        }),
    }
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
        Frame, PickMode, Target, ViewportMode, agent_call, agent_result, caption, markers_op,
        new_token, overlay_op, page_frame, rearm_ops, rect_op, rotated, shoot_ops,
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
                    x: 204.,
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
        assert!((scale - 384. / 852.).abs() < 1e-3 && f.x >= 26. && (f.y - 60.).abs() < 1e-3);
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

    #[test]
    fn ops_after_load_rearms_pick() {
        assert_eq!(rearm_ops(PickMode::Off), Vec::<serde_json::Value>::new());
        assert_eq!(
            rearm_ops(PickMode::Annotate),
            vec![json!({"op": "pick", "mode": "annotate"})]
        );
        assert_eq!(
            rearm_ops(PickMode::Inspect),
            vec![json!({"op": "pick", "mode": "inspect"})]
        );
    }

    #[test]
    fn shots_take_turns() {
        let mut gate = super::ShotGate::default();
        assert!(gate.enter().is_none());
        let mut second = gate.enter().expect("waits");
        let mut third = gate.enter().expect("waits");
        assert!(second.try_recv().unwrap().is_none());
        gate.leave();
        assert!(second.try_recv().unwrap().is_some());
        assert!(third.try_recv().unwrap().is_none());
        gate.leave();
        assert!(third.try_recv().unwrap().is_some());
        gate.leave();
        assert!(gate.enter().is_none());
    }

    #[test]
    fn markers_are_numbered_selectors() {
        assert_eq!(
            markers_op(&[(1, "#a".into()), (3, "b > i".into())]),
            json!({"op": "markers", "items": [
                {"n": 1, "selector": "#a"}, {"n": 3, "selector": "b > i"}]})
        );
        assert_eq!(markers_op(&[]), json!({"op": "markers", "items": []}));
    }

    #[test]
    fn screenshot_wraps_overlay() {
        assert_eq!(overlay_op(true), json!({"op": "overlay", "hidden": true}));
        assert_eq!(shoot_ops(), [overlay_op(true), overlay_op(false)]);
    }

    #[test]
    fn rotating_swaps_the_size() {
        let phone = ViewportMode::Fixed {
            name: Some("iPhone 15".into()),
            width: 393,
            height: 852,
        };
        assert_eq!(
            rotated(&phone),
            Some(ViewportMode::Fixed {
                name: Some("iPhone 15".into()),
                width: 852,
                height: 393,
            })
        );
        assert_eq!(rotated(&ViewportMode::Responsive), None);
    }

    #[test]
    fn pick_modes_are_named() {
        assert_eq!(PickMode::Off.as_str(), "off");
        assert_eq!(PickMode::Annotate.as_str(), "annotate");
        assert_eq!(PickMode::Inspect.as_str(), "inspect");
    }
}
