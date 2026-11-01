//! The browser tab: a WKWebView laid over the tab below a GPUI toolbar (back,
//! forward, reload, address, Web Inspector). It draws above GPUI, so it is
//! placed on the page area under the toolbar (beside the side panel when one
//! is open) each time that is laid out, and hidden whenever another tab or
//! pane takes its place (`Workspace::sync_browser`). Cookies and storage are
//! kept per project.
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

use std::path::{Path, PathBuf};
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
    BrowserOp, ConsoleEntry, Device, Logs, PageEvent, Target, ViewportMode, device_by_name, fit,
    normalize_address, page_script, parse_ipc, resize_mode, search_url,
};
use sawit_core::probe;
use serde_json::{Value, json};

pub use annotate::notes_lost;
use annotate::{AnnotateEvent, AnnotatePanel, group_order, padded_rect};
use design::{DesignEvent, DesignPanel, Segment};
use engine::{Engine, EngineEvent, EngineSpec, Frame};

use crate::rt;
use crate::states::Placeholder;

/// The tab's icon and title.
pub const ICON: Lucide = Lucide::Globe;
pub const TITLE: &str = "Browser";

actions!(browser, [ToggleAnnotate]);

/// ⇧⌘. toggles Annotate, through this binding alone, the keymap's: the page's
/// overlay answers no one's, and Sawit answers an agent's (`own_key`). A Mac
/// reports ⇧⌘. as `cmd->` (Shift off, the character it types), so that is what
/// is bound, as the canvas binds its shifted keys; `cmd-shift-.` would never
/// match. The keymap editor and the tooltip write `cmd->` as ⇧⌘.
/// (`keymap::names`).
///
/// GPUI sees the ⌘ keys before the page does, even while the page has AppKit's
/// keyboard, and runs them where GPUI's keyboard is: the view's own while the
/// page has the keyboard ([`BrowserView::give_page_keyboard`], and a press in
/// the page). So the Annotate key works in the page too when it has ⌘; a key
/// without ⌘ types into the page. ⌘C is let through there, as the terminal and
/// the canvas let it through, past gpui-kit's root, which would copy its own
/// text selection: the Edit menu then copies in the page (`cli::set_menus`).
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd->", ToggleAnnotate, Some("Browser")),
        KeyBinding::new("cmd-c", NoAction, Some("Browser")),
    ]);
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
}

/// What picking does while `panel` is open: Design inspects only on its Inspect segment,
/// so clicks reach the page on the others.
fn pick_mode(panel: Panel, segment: Segment) -> PickMode {
    match (panel, segment) {
        (Panel::Annotate, _) => PickMode::Annotate,
        (Panel::Design, Segment::Inspect) => PickMode::Inspect,
        (Panel::Design, _) => PickMode::Off,
    }
}

/// Whether a pick the page reports (`mode` as it says) is one Sawit armed: the page can arm
/// the picker itself, so a pick counts only in the mode armed, with its panel open.
fn pick_wanted(panel: Option<Panel>, pick: PickMode, mode: &str) -> bool {
    match mode {
        "annotate" => panel == Some(Panel::Annotate) && pick == PickMode::Annotate,
        "inspect" => panel == Some(Panel::Design) && pick == PickMode::Inspect,
        _ => false,
    }
}

/// A key an agent presses (`browser_press_key`) that Sawit answers instead of the page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OwnKey {
    /// ⇧⌘.: toggles Annotate.
    ToggleAnnotate,
    /// Esc: stops picking, while picking.
    Escape,
}

/// Whether `key` (`Meta+Shift+.`, as `browser_press_key` takes it) is ⇧⌘. or Esc. The page
/// answers only the user's own Esc while picking, and no one's ⇧⌘. (a page can dispatch keys,
/// ⇧⌘. opens Annotate and arms the picker, and the user's own is the keymap's), so an agent's
/// go to Sawit. `>` is ⇧. on the keyboard.
fn own_key(key: &str) -> Option<OwnKey> {
    let parts: Vec<String> = key
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    let (key, mods) = parts.split_last()?;
    let meta = mods
        .iter()
        .any(|m| matches!(m.as_str(), "meta" | "cmd" | "command"));
    let shift = mods.iter().any(|m| m == "shift");
    let others = mods
        .iter()
        .any(|m| !matches!(m.as_str(), "meta" | "cmd" | "command" | "shift"));
    match key.as_str() {
        "." if meta && shift && !others => Some(OwnKey::ToggleAnnotate),
        ">" if meta && !others => Some(OwnKey::ToggleAnnotate),
        // As the picker takes it: any Esc.
        "escape" | "esc" => Some(OwnKey::Escape),
        _ => None,
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

/// How long a shot waits for the overlay to hide, and then for WebKit's capture, so a
/// lost callback can't keep the overlay hidden and the next shots waiting.
const SHOT_STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// How long a pick waits for its screenshot (its turn included) before adding its row
/// without one.
const PICK_SHOT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// `fut`'s output, or `None` when `timer` fires first.
async fn within<F: std::future::Future + Unpin>(
    fut: F,
    timer: impl std::future::Future<Output = ()> + Unpin,
) -> Option<F::Output> {
    match futures::future::select(fut, timer).await {
        futures::future::Either::Left((out, _)) => Some(out),
        futures::future::Either::Right(_) => None,
    }
}

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
    /// The device menu is open, over the page: the webview is hidden meanwhile
    /// ([`Self::set_menu_open`]).
    menu_open: bool,
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
    /// Keeps the "Browser" key context on the dispatch path (⇧⌘.), and GPUI's
    /// keyboard while the page has AppKit's.
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
    /// Links to other apps the page opened: asked about one at a time.
    external: ExternalGate,
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
                |this, address, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => {
                        let typed = address.read(cx).value().to_string();
                        this.navigate(&typed, cx);
                        this.give_page_keyboard(window, cx);
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
            menu_open: false,
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
            external: ExternalGate::default(),
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

    /// Gives the keyboard to the view (not the page), so ⇧⌘. reaches it.
    fn take_keyboard(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(engine) = &self.engine {
            engine.focus_parent();
        }
        self.focus.focus(window, cx);
    }

    /// The project whose browser this is; `None` for the browser of no project.
    pub fn project(&self) -> Option<&Path> {
        self.project.as_deref()
    }

    pub fn panel(&self) -> Option<Panel> {
        self.panel
    }

    /// Opens `panel`, or closes it when it is the one open. Annotate arms
    /// element picking, Design inspecting (on its Inspect segment); closing disarms.
    pub fn toggle_panel(&mut self, panel: Panel, cx: &mut Context<Self>) {
        if self.panel == Some(panel) {
            self.panel = None;
            self.set_pick(PickMode::Off, cx);
        } else {
            self.panel = Some(panel);
            let segment = self.design_panel.read(cx).segment();
            self.set_pick(pick_mode(panel, segment), cx);
            self.scan_if_design_open(cx);
        }
        cx.notify();
    }

    /// ⇧⌘.: like the Annotate button, except that with Annotate open after Esc
    /// it picks again instead of closing.
    fn annotate_key(&mut self, cx: &mut Context<Self>) {
        if self.panel == Some(Panel::Annotate) && self.pick == PickMode::Off {
            self.set_pick(PickMode::Annotate, cx);
        } else {
            self.toggle_panel(Panel::Annotate, cx);
        }
    }

    /// An agent's ⇧⌘., or Esc while picking: does what the user's key does and answers
    /// where that left the panel and the picker. `None` for a key that is the page's.
    fn press_own_key(&mut self, key: &str, cx: &mut Context<Self>) -> Option<Value> {
        match own_key(key)? {
            OwnKey::ToggleAnnotate => self.annotate_key(cx),
            OwnKey::Escape if self.pick != PickMode::Off => self.set_pick(PickMode::Off, cx),
            OwnKey::Escape => return None,
        }
        Some(json!({
            "pressed": key,
            "panel": self.panel.map(Panel::as_str),
            "picking": self.pick.as_str(),
        }))
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

    /// Annotation notes that would be lost with the tab: not copied, sent or read by an agent.
    pub fn unsent_notes(&self, cx: &App) -> usize {
        self.annotate.read(cx).unsent(cx)
    }

    /// Shows `text` in the Annotate footer for a moment.
    pub fn set_annotate_note(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.annotate.update(cx, |a, cx| a.set_note(text, cx));
    }

    fn on_design(&mut self, event: &DesignEvent, cx: &mut Context<Self>) {
        match event {
            // Choosing Inspect again after Esc picks again.
            DesignEvent::Segment(segment) => {
                if self.panel == Some(Panel::Design) {
                    self.set_pick(pick_mode(Panel::Design, *segment), cx);
                }
            }
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
                    self.annotate.update(cx, |a, cx| {
                        a.mark_sent(cx);
                        a.set_note("Copied", cx);
                    });
                }
            }
            AnnotateEvent::Send => {
                if let Some(md) = self.annotations_markdown(cx) {
                    // Pasted, or copied when no terminal takes it: either way the user has them.
                    self.annotate.update(cx, |a, cx| a.mark_sent(cx));
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
            // Shots wait their turn; a row without a screenshot beats no row.
            let timer = cx.background_executor().timer(PICK_SHOT_TIMEOUT);
            let shot = match within(rx, timer).await {
                Some(shot) => shot.unwrap_or_else(|_| Err("The screenshot was cancelled.".into())),
                None => Err("The screenshot took too long.".into()),
            };
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
            let unreachable = sawit_core::browser::UNREACHABLE;
            let _ = tx.send(Err(format!("{unreachable}: {e}")));
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
            // A page without an agent has no overlay to hide; one that doesn't answer
            // is shot as it is.
            if let Ok(hidden) = hidden {
                let timer = cx.background_executor().timer(SHOT_STEP_TIMEOUT);
                let _ = within(hidden, timer).await;
            }
            let (tx, rx) = oneshot::channel();
            let started = this.update(cx, |this, _| this.snap(css_rect, tx));
            let result = if started.is_ok() {
                let timer = cx.background_executor().timer(SHOT_STEP_TIMEOUT);
                match within(rx, timer).await {
                    Some(shot) => {
                        shot.unwrap_or_else(|_| Err("The screenshot was cancelled.".into()))
                    }
                    None => Err(format!(
                        "WebKit didn't capture the page within {} s.",
                        SHOT_STEP_TIMEOUT.as_secs()
                    )),
                }
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

    /// The tab is opened again: a port clicked in Ports (`url`) opens there, the Browser
    /// button (none) keeps the page.
    pub fn show(&mut self, url: Option<&str>, cx: &mut Context<Self>) {
        if let Some(url) = url {
            self.navigate(url, cx);
        }
    }

    /// Opens what the user typed (or an agent asked for). The keyboard stays where it is: an
    /// agent's load, or the dev server found at start, never takes it from the user.
    fn navigate(&mut self, typed: &str, cx: &mut Context<Self>) {
        // Opened by the user or an agent: the page's links to other apps may ask again.
        self.external.reset();
        self.load(address_url(typed), cx);
    }

    /// Loads `url` in the tab, as it is.
    fn load(&mut self, url: String, cx: &mut Context<Self>) {
        self.navigated = true;
        self.loading = true;
        self.url = url.clone();
        if let Some(engine) = &self.engine
            && let Err(e) = engine.load_url(&url)
        {
            eprintln!("sawit: {e}");
        }
        cx.notify();
    }

    /// Gives the page the keyboard, while the tab shows: AppKit's, and GPUI's,
    /// which goes to the view itself. GPUI sees the ⌘ keys first, wherever
    /// AppKit's keyboard is, and would otherwise run them where its own keyboard
    /// was: ⌘V pasting into the address field, or into the terminal.
    fn give_page_keyboard(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.page_shown() {
            return;
        }
        if let Some(engine) = &self.engine {
            engine.focus();
        }
        self.focus.focus(window, cx);
    }

    fn receive(&mut self, message: FromPage, window: &mut Window, cx: &mut Context<Self>) {
        match message {
            FromPage::Event(PageEvent::Console(entry)) => self.logs.push_console(entry),
            FromPage::Event(PageEvent::Network(entry)) => self.logs.push_network(entry),
            FromPage::Event(PageEvent::Picked(pick)) => {
                // A page can arm the picker itself: only picks Sawit asked for count.
                if !pick_wanted(self.panel, self.pick, pick["mode"].as_str().unwrap_or("")) {
                    return;
                }
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
            // From the page: Esc while picking. ⇧⌘. never comes from it (browser_overlay.js):
            // the user's own is the keymap's, and Sawit answers an agent's itself.
            FromPage::Event(PageEvent::Key(key)) => {
                if key == "escape" {
                    self.set_pick(PickMode::Off, cx);
                }
            }
            // The user's press in the page gave it AppKit's keyboard: GPUI's follows. Never
            // while the tab is hidden: nobody can press in a hidden page, and the keyboard
            // would leave the terminal or field that has it for a view that isn't drawn, which
            // the workspace then takes it from (`workspace::KeyboardHome`).
            FromPage::Event(PageEvent::Pressed) => {
                if self.page_shown() {
                    self.focus.focus(window, cx);
                }
            }
            FromPage::Event(PageEvent::Url(url)) => {
                // The page says the address changed; WebKit says what it is.
                self.url = self
                    .engine
                    .as_ref()
                    .and_then(|e| e.url())
                    .filter(|u| !u.is_empty())
                    .unwrap_or(url);
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
            FromPage::Engine(EngineEvent::NewWindow(url)) => {
                match new_window_action(&url, &self.current_url()) {
                    NewWindow::Ignore => {}
                    NewWindow::Load(url) => self.load(url, cx),
                    NewWindow::External(url) => self.open_external(url, window, cx),
                }
            }
            FromPage::Engine(EngineEvent::External(url)) => self.open_external(url, window, cx),
            FromPage::Shoot { rect, reply } => self.shoot(rect, reply, cx),
        }
    }

    /// The page went to a link another app handles: asks the user before opening it, one
    /// link at a time; after a "Cancel" the page asks no more until the user or an agent opens
    /// a page or reloads it, which the question says.
    fn open_external(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.external.ask() {
            return;
        }
        let shown = match url.char_indices().nth(EXTERNAL_SHOWN) {
            Some((i, _)) => format!("{}…", &url[..i]),
            None => url.clone(),
        };
        let answer = window.prompt(
            PromptLevel::Info,
            "Open this link in another app?",
            Some(&format!(
                "The page asked to open {shown}. If you cancel, the page's links to other apps do \
                 nothing until you reload it or open another page."
            )),
            &["Open", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            let open = matches!(answer.await, Ok(0));
            let _ = this.update(cx, |this, cx| {
                this.external.answered(open);
                if open {
                    // NSWorkspace: no process to wait for.
                    cx.open_url(&url);
                }
            });
        })
        .detach();
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
        // Sawit's keys, not the page's (see `own_key`): no page needed.
        if let BrowserOp::Press { key } = &op
            && let Some(answer) = self.press_own_key(key, cx)
        {
            let _ = tx.send(Ok(answer));
            return rx;
        }
        // Reloaded by the user (the toolbar) or an agent: the page's links to other apps may ask
        // again, as after opening a page. A page's own reload doesn't come this way.
        if op == BrowserOp::Reload {
            self.external.reset();
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
            BrowserOp::Evaluate { script } => {
                evaluate(Rc::downgrade(&engine), script, reply);
                return;
            }
            BrowserOp::Annotations { clear } => {
                let markdown = self.annotations_markdown(cx);
                let count = self.annotate.read(cx).annotations(cx).len();
                self.annotate.update(cx, |a, cx| {
                    if clear {
                        a.clear(cx);
                    } else {
                        a.mark_sent(cx);
                    }
                });
                json!({ "markdown": markdown, "count": count })
            }
            BrowserOp::Inspect { target, format } => {
                let target = serde_json::to_value(&target).unwrap_or_default();
                return self.inspect_op(target, format, reply, cx);
            }
            BrowserOp::Design { part } => return self.design_op(part, reply, cx),
            BrowserOp::ExportDesign {
                include_videos,
                replace,
            } => {
                let _ = reply.send(self.export_op(include_videos, replace, cx));
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
    /// be hidden whenever another tab or pane takes its place. Hidden, it gives
    /// AppKit's keyboard back to the window. GPUI's, if the view or a field in it
    /// had it (after a press in the page), goes to the workspace once the view is
    /// no longer drawn (`workspace::KeyboardHome`). Shown again, the address
    /// field has GPUI's back if it had it; the page has neither until a click in it.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        let was = self.page_shown();
        self.visible = visible;
        // A menu left open goes with the toolbar that drew it, which says nothing then.
        if !visible {
            self.menu_open = false;
        }
        self.show_page(was);
    }

    /// The device menu opened or closed. It opens over the page, and the webview draws above
    /// GPUI's drawing, menus too: the menu would open unseen behind the page, the button alone
    /// showing it open. So the webview is hidden while the menu is open, and comes back, at the
    /// size picked, when it closes.
    fn set_menu_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.menu_open == open {
            return;
        }
        let was = self.page_shown();
        self.menu_open = open;
        self.show_page(was);
        cx.notify();
    }

    /// Whether the webview shows: its tab is on screen, and no menu is open over it.
    fn page_shown(&self) -> bool {
        self.visible && !self.menu_open
    }

    /// Shows or hides the webview, when [`Self::page_shown`] is no longer `was`.
    fn show_page(&self, was: bool) {
        let shown = self.page_shown();
        if shown == was {
            return;
        }
        if let Some(engine) = &self.engine {
            engine.set_visible(shown);
            if !shown {
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
                visible: self.page_shown(),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            // The page hidden under the device menu leaves the same backdrop as a device's.
            .when(device.is_some() || self.menu_open, |d| {
                d.bg(cx.theme().muted)
            })
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
            .on_action(cx.listener(|this, _: &ToggleAnnotate, _, cx| {
                this.annotate_key(cx);
            }))
            .size_full()
            .child(self.toolbar(window, cx))
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

/// What the address field (and an agent's navigate) loads for `typed`: its address when
/// WebKit can read it, else a search for the text (`file://my host/x` isn't a URL).
fn address_url(typed: &str) -> String {
    let url = normalize_address(typed);
    if engine::loadable(&url) {
        url
    } else {
        search_url(typed)
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

/// What a page's request for a new window (`window.open`, a `target=_blank` link) does.
#[derive(Debug, PartialEq)]
enum NewWindow {
    Ignore,
    /// Opens in the tab.
    Load(String),
    /// A link another app handles: asked about.
    External(String),
}

/// What to do with the URL WebKit gives for a new window, from the page at `current`. It is
/// a URL already, so it isn't read as typed text: web pages load as they are; an empty or
/// `about:` window (`window.open()`, then `w.location = …`) is left alone, since the tab has
/// only one page; `data:` and `javascript:` are refused, as a page's own navigation to them
/// is; a local file only from a local page, a `blob:` only from its own origin; any other
/// scheme belongs to another app.
fn new_window_action(url: &str, current: &str) -> NewWindow {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once(':').filter(|(s, _)| {
        s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }) else {
        return NewWindow::Ignore;
    };
    let ok = match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => true,
        "blob" => origin(rest).is_some_and(|o| Some(o) == origin(current)),
        "file" => current.to_ascii_lowercase().starts_with("file:"),
        "about" | "data" | "javascript" => false,
        _ => return NewWindow::External(url.to_string()),
    };
    if ok && engine::loadable(url) {
        NewWindow::Load(url.to_string())
    } else {
        NewWindow::Ignore
    }
}

/// An http(s) URL's `scheme://host:port`, lowercased.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    Some(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

/// How much of a link to another app the prompt shows.
const EXTERNAL_SHOWN: usize = 200;

/// Asks about a page's links to other apps one at a time, and stops asking once the user
/// says no, so a page can neither launch apps unasked nor pile up prompts.
#[derive(Default)]
struct ExternalGate {
    asking: bool,
    refused: bool,
}

impl ExternalGate {
    /// Whether to ask about a link now; while asking, or after a "Cancel", it is dropped.
    fn ask(&mut self) -> bool {
        if self.asking || self.refused {
            return false;
        }
        self.asking = true;
        true
    }

    fn answered(&mut self, open: bool) {
        self.asking = false;
        self.refused = !open;
    }

    /// The user or an agent opened a page, or reloaded it: its links may ask again.
    fn reset(&mut self) {
        self.refused = false;
    }
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

/// The device `mode` imitates, and whether it is turned on its side (the size swapped):
/// `None` in responsive mode, or for a size of no device's.
fn device_of(mode: &ViewportMode) -> Option<(&'static Device, bool)> {
    let ViewportMode::Fixed {
        name: Some(name),
        width,
        height,
    } = mode
    else {
        return None;
    };
    let device = device_by_name(name)?;
    match (*width, *height) {
        size if size == (device.width, device.height) => Some((device, false)),
        size if size == (device.height, device.width) => Some((device, true)),
        _ => None,
    }
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

/// Runs an agent's `browser_evaluate` script: as a function body when only that form
/// compiles (`evaluate_as_body` knows before WebKit compiles anything, whose `SyntaxError`
/// would reach the page as an error of its own), else as an expression, and as a body when
/// WebKit can't compile that either. WebKit compiles it (`evaluate_call`), so it runs on pages
/// whose Content-Security-Policy forbids `eval`.
fn evaluate(engine: std::rc::Weak<Engine>, script: String, reply: oneshot::Sender<OpResult>) {
    let Some(first) = engine.upgrade() else {
        let _ = reply.send(Err("The browser isn't open.".into()));
        return;
    };
    let answer = |result: Result<Option<String>, String>| {
        result.and_then(|text| agent_answer(text.as_deref().unwrap_or_default()))
    };
    let body = sawit_core::browser::evaluate_as_body(&script);
    first.eval_text(
        &sawit_core::browser::evaluate_call(&script, body),
        move |result| match result {
            Err(e) if !body && e.contains("SyntaxError") => match engine.upgrade() {
                Some(engine) => engine.eval_text(
                    &sawit_core::browser::evaluate_call(&script, true),
                    move |result| {
                        let _ = reply.send(answer(result));
                    },
                ),
                None => {
                    let _ = reply.send(Err("The browser was closed.".into()));
                }
            },
            result => {
                let _ = reply.send(answer(result));
            }
        },
    );
}

/// The agent's answer as WebKit hands it over: its return value (a JSON
/// string) serialized as JSON once more.
fn agent_result(result: &str) -> OpResult {
    let text: String = serde_json::from_str(result).unwrap_or_default();
    agent_answer(&text)
}

/// The agent's answer: `{"ok": true, …}`, or `{"ok": false, "error"}`; nothing when the page
/// has no agent.
fn agent_answer(text: &str) -> OpResult {
    if text.is_empty() {
        return Err(sawit_core::browser::NOT_READY.into());
    }
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| format!("The page sent a bad answer: {e}"))?;
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
        AnnotateEvent, Frame, Panel, PickMode, Segment, Target, ViewportMode, agent_call,
        agent_result, caption, markers_op, new_token, overlay_op, page_frame, pick_mode, rearm_ops,
        rect_op, rotated, shoot_ops, within,
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

    /// Whatever is typed (or an agent asks for) is loaded as a URL WebKit reads, or searched:
    /// wry unwraps the `NSURL` it makes of the address, so anything else aborted Sawit.
    #[test]
    fn every_address_is_one_webkit_reads() {
        for typed in [
            "localhost:3000:3000",
            "http://localhost 3000",
            "https://my site.com",
            "https://foo:exa%7Cmple.com",
            "https://::1",
            "file://my host/x",
            "file://a:1:2/x",
            "about://a:1:2",
            "localhost:5173",
            "example.com/a b",
            "https://example.com/a b",
            "file:///tmp/a b.html",
            "mailto:a@b.c",
            "rust gpui",
            "",
        ] {
            let url = super::address_url(typed);
            assert!(super::engine::loadable(&url), "{typed} → {url}");
        }
        for bad in [
            "https://localhost:3000:3000",
            "https://my site.com",
            "file://my host/x",
        ] {
            assert!(!super::engine::loadable(bad), "{bad}");
        }
        assert_eq!(
            super::address_url("file://my host/x"),
            sawit_core::browser::search_url("file://my host/x")
        );
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

    /// `window.open()` and `target=_blank` links: web pages open in the tab as WebKit gives
    /// them; a blank window (`window.open()`, then `w.location = …`) leaves the page alone;
    /// links to other apps are asked about; a local file opens only from a local page, and a
    /// page's own `blob:` only from that page.
    #[test]
    fn new_windows() {
        use super::NewWindow::{External, Ignore, Load};
        use super::new_window_action as act;
        let page = "http://localhost:5173/cart";
        let load = |u: &str| Load(u.to_string());
        let ext = |u: &str| External(u.to_string());
        for (url, want) in [
            ("", Ignore),
            ("about:blank", Ignore),
            ("about:srcdoc", Ignore),
            (
                "https://accounts.example/login?next=/",
                load("https://accounts.example/login?next=/"),
            ),
            ("http://localhost:5173/x", load("http://localhost:5173/x")),
            ("mailto:a@b.c", ext("mailto:a@b.c")),
            ("tel:+15551234", ext("tel:+15551234")),
            ("slack://open?team=T1", ext("slack://open?team=T1")),
            // `localhost:` is a scheme here, as WebKit read it: no app has it.
            ("localhost:3000:3000", ext("localhost:3000:3000")),
            ("file:///etc/hosts", Ignore),
            ("data:text/html,<p>hi</p>", Ignore),
            ("javascript:alert(1)", Ignore),
            (
                "blob:http://localhost:5173/9d2c",
                load("blob:http://localhost:5173/9d2c"),
            ),
            ("blob:https://other.example/9d2c", Ignore),
            ("https://my site.com", Ignore),
        ] {
            assert_eq!(act(url, page), want, "{url:?}");
        }
        assert_eq!(
            act("file:///tmp/b.html", "file:///tmp/a.html"),
            load("file:///tmp/b.html")
        );
    }

    #[test]
    fn links_to_other_apps_are_asked_about_one_at_a_time() {
        let mut gate = super::ExternalGate::default();
        assert!(gate.ask(), "the first link asks");
        assert!(!gate.ask(), "a second, while asking, is dropped");
        gate.answered(true);
        assert!(gate.ask(), "after an Open, the next link asks");
        gate.answered(false);
        assert!(!gate.ask(), "after a Cancel, the page asks no more");
        gate.reset();
        assert!(gate.ask(), "until a page is opened");
    }

    /// A browser tab that is never drawn (so it has no webview), in a test window.
    fn test_browser(
        cx: &mut gpui_kit::TestAppContext,
    ) -> (
        gpui_kit::Entity<super::BrowserView>,
        &mut gpui_kit::VisualTestContext,
    ) {
        use gpui_kit::AppContext as _;
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let cx = cx.add_empty_window();
        let view =
            cx.update(|window, cx| cx.new(|cx| super::BrowserView::new(None, None, window, cx)));
        (view, cx)
    }

    /// What the engine reports, as the view gets it.
    fn engine_event(
        view: &gpui_kit::Entity<super::BrowserView>,
        event: super::EngineEvent,
        cx: &mut gpui_kit::VisualTestContext,
    ) {
        cx.update(|window, cx| {
            view.update(cx, |b, cx| {
                b.receive(super::FromPage::Engine(event), window, cx)
            })
        });
        cx.run_until_parked();
    }

    /// A page script can arm the picker and click for itself (`__sawitAgent` is the page's to
    /// call): a pick adds an annotation only while the user has Annotate open and picking.
    #[gpui_kit::test]
    fn picks_count_only_while_annotating(cx: &mut gpui_kit::TestAppContext) {
        use sawit_core::browser::PageEvent;
        let (view, cx) = test_browser(cx);
        let pick = serde_json::json!({"mode": "annotate", "element": {
            "selector": "#buy", "label": "button#buy", "tag": "button",
            "rect": {"x": 1, "y": 1, "width": 40, "height": 20}}});
        let picked = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                view.update(cx, |b, cx| {
                    b.receive(
                        super::FromPage::Event(PageEvent::Picked(pick.clone())),
                        window,
                        cx,
                    )
                })
            });
            cx.run_until_parked();
        };
        let rows = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| view.read(cx).annotate.read(cx).annotations(cx).len())
        };
        picked(cx);
        assert_eq!(rows(cx), 0, "Annotate is closed");

        cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(super::Panel::Annotate, cx)));
        picked(cx);
        assert_eq!(rows(cx), 1, "the user's pick");

        // Esc stopped picking; the panel is still open.
        cx.update(|_, cx| view.update(cx, |b, cx| b.set_pick(super::PickMode::Off, cx)));
        picked(cx);
        assert_eq!(rows(cx), 1, "picking is off");

        // Design's Inspect picks are no annotations either.
        cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(super::Panel::Design, cx)));
        picked(cx);
        assert_eq!(rows(cx), 1, "Design is open");
    }

    /// The page answers no one's ⇧⌘. (a page can dispatch one, and the user's is the keymap's),
    /// so an agent's `browser_press_key "Meta+Shift+."` is Sawit's to answer: it toggles
    /// Annotate as the user's key does, before the page has loaded too. Esc while picking stops picking as
    /// the user's does; other keys, and Esc while not picking, go to the page.
    #[gpui_kit::test]
    fn an_agents_shift_cmd_period_is_sawits_to_handle(cx: &mut gpui_kit::TestAppContext) {
        use sawit_core::browser::BrowserOp;
        let (view, cx) = test_browser(cx);
        let press = |key: &str, cx: &mut gpui_kit::VisualTestContext| {
            let mut answer = cx.update(|_, cx| {
                view.update(cx, |b, cx| {
                    b.apply(BrowserOp::Press { key: key.into() }, cx)
                })
            });
            cx.run_until_parked();
            answer.try_recv().ok().flatten()
        };
        let state = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| {
                let b = view.read(cx);
                (b.panel(), b.pick)
            })
        };

        let answer = press("Meta+Shift+.", cx);
        assert_eq!(state(cx), (Some(Panel::Annotate), PickMode::Annotate));
        assert_eq!(
            answer,
            Some(Ok(
                json!({"pressed": "Meta+Shift+.", "panel": "annotate", "picking": "annotate"})
            ))
        );
        press("Escape", cx);
        assert_eq!(
            state(cx),
            (Some(Panel::Annotate), PickMode::Off),
            "Esc stops picking"
        );
        press("Cmd+Shift+.", cx);
        assert_eq!(
            state(cx),
            (Some(Panel::Annotate), PickMode::Annotate),
            "picks again"
        );
        press("shift+meta+>", cx);
        assert_eq!(state(cx), (None, PickMode::Off), "and closes");
        // The page's: waits for it (this tab never loads one).
        assert_eq!(press("Escape", cx), None);
        assert_eq!(press("Meta+.", cx), None);
        assert_eq!(state(cx), (None, PickMode::Off));
    }

    #[test]
    fn sawits_own_keys() {
        use super::{OwnKey, own_key};
        for key in [
            "Meta+Shift+.",
            "Shift+Meta+.",
            "cmd+shift+.",
            "Command+Shift+>",
            "Meta+>",
            " Meta + Shift + . ",
        ] {
            assert_eq!(own_key(key), Some(OwnKey::ToggleAnnotate), "{key}");
        }
        for key in ["Escape", "esc", "Shift+Escape"] {
            assert_eq!(own_key(key), Some(OwnKey::Escape), "{key}");
        }
        for key in [
            ".",
            "Meta+.",
            "Shift+.",
            "Ctrl+Meta+Shift+.",
            "Meta+Alt+Shift+.",
            "Meta+Shift+,",
            "Meta+Shift+a",
            "Enter",
            "",
        ] {
            assert_eq!(own_key(key), None, "{key}");
        }
    }

    /// A note typed on an annotation holds the tab open (closing it asks) until it is copied
    /// or sent; a note typed after that holds it again.
    #[gpui_kit::test]
    fn unsent_notes_hold_the_tab(cx: &mut gpui_kit::TestAppContext) {
        use sawit_core::browser::PageEvent;
        let (view, cx) = test_browser(cx);
        cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(super::Panel::Annotate, cx)));
        let pick = serde_json::json!({"mode": "annotate", "element": {
            "selector": "#buy", "label": "button#buy", "tag": "button",
            "rect": {"x": 1, "y": 1, "width": 40, "height": 20}}});
        cx.update(|window, cx| {
            view.update(cx, |b, cx| {
                b.receive(super::FromPage::Event(PageEvent::Picked(pick)), window, cx)
            })
        });
        cx.run_until_parked();
        let unsent = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| view.read(cx).unsent_notes(cx))
        };
        assert_eq!(unsent(cx), 0, "a row with no note");
        let note = |text: &str, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                let panel = view.read(cx).annotate.clone();
                panel.update(cx, |p, cx| p.type_note(0, text, window, cx));
            })
        };
        note("Make it blue", cx);
        assert_eq!(unsent(cx), 1);
        cx.update(|_, cx| view.update(cx, |b, cx| b.on_annotate(&AnnotateEvent::Copy, cx)));
        assert_eq!(unsent(cx), 0, "copied");
        note("Make it blue, and bigger", cx);
        assert_eq!(unsent(cx), 1, "changed since");
        cx.update(|_, cx| view.update(cx, |b, cx| b.on_annotate(&AnnotateEvent::Send, cx)));
        assert_eq!(unsent(cx), 0, "sent");
    }

    /// A page's link to another app (a `mailto:`, or an iframe's `slack://` with no click)
    /// opens only when the user says so, through the system (no process left behind); while
    /// asked, and after a Cancel, the page's further links are dropped.
    #[gpui_kit::test]
    fn links_to_other_apps_open_only_when_the_user_agrees(cx: &mut gpui_kit::TestAppContext) {
        use super::EngineEvent::External;
        let (view, cx) = test_browser(cx);
        engine_event(&view, External("mailto:a@b.c".into()), cx);
        assert!(cx.has_pending_prompt(), "the user is asked");
        assert_eq!(cx.opened_url(), None, "nothing opens unasked");
        engine_event(&view, External("slack://open?team=T1".into()), cx);
        cx.simulate_prompt_answer("Open");
        cx.run_until_parked();
        assert_eq!(cx.opened_url().as_deref(), Some("mailto:a@b.c"));
        assert!(
            !cx.has_pending_prompt(),
            "the link sent while asking was dropped"
        );

        engine_event(&view, External("zoommtg://join?confno=1".into()), cx);
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        engine_event(&view, External("zoommtg://join?confno=2".into()), cx);
        assert!(
            !cx.has_pending_prompt(),
            "after a Cancel the page asks no more"
        );
        assert_eq!(cx.opened_url().as_deref(), Some("mailto:a@b.c"));

        cx.update(|_, cx| view.update(cx, |b, cx| b.navigate("example.com", cx)));
        engine_event(&view, External("zoommtg://join?confno=3".into()), cx);
        assert!(
            cx.has_pending_prompt(),
            "a page the user opened may ask again"
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
    }

    /// After a Cancel the page's further links to other apps are dropped, so the question says
    /// so, and how to have them ask again: they were dropped without a word, and looked dead.
    /// A reload the user or an agent asks for lets them ask again, as opening a page does; a
    /// page's own reload doesn't come this way, so it can't loop the question.
    #[gpui_kit::test]
    fn after_a_cancel_a_reload_lets_links_ask_again(cx: &mut gpui_kit::TestAppContext) {
        use super::EngineEvent::External;
        use sawit_core::browser::BrowserOp;
        let (view, cx) = test_browser(cx);
        engine_event(&view, External("zoommtg://join?confno=1".into()), cx);
        let (_, detail) = cx.pending_prompt().expect("the user is asked");
        assert!(detail.contains("zoommtg://join?confno=1"), "{detail}");
        assert!(
            detail.contains("reload"),
            "Cancel isn't explained: {detail}"
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        engine_event(&view, External("zoommtg://join?confno=2".into()), cx);
        assert!(
            !cx.has_pending_prompt(),
            "after a Cancel the page asks no more"
        );

        cx.update(|_, cx| view.update(cx, |b, cx| drop(b.apply(BrowserOp::Reload, cx))));
        engine_event(&view, External("zoommtg://join?confno=3".into()), cx);
        assert!(cx.has_pending_prompt(), "a reload lets the page ask again");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
    }

    /// A port clicked in Ports opens in the browser even when it is open already: it used to
    /// come forward still on the page it had.
    #[gpui_kit::test]
    fn an_open_browser_goes_to_the_port_clicked(cx: &mut gpui_kit::TestAppContext) {
        let (view, cx) = test_browser(cx);
        let url =
            |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| view.read(cx).url.clone());
        cx.update(|_, cx| view.update(cx, |b, cx| b.show(Some("http://localhost:5173"), cx)));
        assert_eq!(url(cx), "http://localhost:5173");
        cx.update(|_, cx| view.update(cx, |b, cx| b.show(Some("http://localhost:3000"), cx)));
        assert_eq!(url(cx), "http://localhost:3000");
        // The Browser button: the page stays.
        cx.update(|_, cx| view.update(cx, |b, cx| b.show(None, cx)));
        assert_eq!(url(cx), "http://localhost:3000");
    }

    /// A blank new window (`window.open()`, from a timer) leaves the page under test as it is,
    /// and a `mailto:` popup asks before Mail opens instead of loading `https://mailto:…`.
    #[gpui_kit::test]
    fn new_windows_leave_the_page_alone(cx: &mut gpui_kit::TestAppContext) {
        use super::EngineEvent::NewWindow;
        let (view, cx) = test_browser(cx);
        cx.update(|_, cx| view.update(cx, |b, cx| b.navigate("localhost:5173/form", cx)));
        let url =
            |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| view.read(cx).url.clone());
        assert_eq!(url(cx), "http://localhost:5173/form");
        engine_event(&view, NewWindow(String::new()), cx);
        assert_eq!(url(cx), "http://localhost:5173/form");
        assert!(!cx.has_pending_prompt());
        engine_event(&view, NewWindow("mailto:a@b.c".into()), cx);
        assert_eq!(url(cx), "http://localhost:5173/form");
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        engine_event(&view, NewWindow("https://a.example/x".into()), cx);
        assert_eq!(url(cx), "https://a.example/x");
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
    fn a_wait_ends_when_its_timer_fires() {
        use futures::executor::block_on;
        use futures::future::{pending, ready};
        let never = pending::<u8>();
        assert_eq!(block_on(within(never, ready(()))), None);
        assert_eq!(block_on(within(ready(7), pending::<()>())), Some(7));
        let (tx, rx) = futures::channel::oneshot::channel::<u8>();
        drop(tx);
        assert!(matches!(
            block_on(within(rx, pending::<()>())),
            Some(Err(_))
        ));
    }

    #[test]
    fn design_picks_only_on_inspect() {
        assert_eq!(
            pick_mode(Panel::Design, Segment::Inspect),
            PickMode::Inspect
        );
        for s in [
            Segment::Colors,
            Segment::Fonts,
            Segment::Assets,
            Segment::System,
        ] {
            assert_eq!(pick_mode(Panel::Design, s), PickMode::Off, "{s:?}");
            assert_eq!(pick_mode(Panel::Annotate, s), PickMode::Annotate);
        }
        assert_eq!(
            pick_mode(Panel::Annotate, Segment::Inspect),
            PickMode::Annotate
        );
    }

    #[test]
    fn pick_modes_are_named() {
        assert_eq!(PickMode::Off.as_str(), "off");
        assert_eq!(PickMode::Annotate.as_str(), "annotate");
        assert_eq!(PickMode::Inspect.as_str(), "inspect");
    }

    /// The Annotate button's tooltip says the key the keymap gives Annotate: ⇧⌘. by default, the
    /// user's own once they change it, and none once they take that out too.
    #[gpui_kit::test]
    fn the_annotate_tooltip_follows_the_keymap(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::{Action, KeyBinding, KeyBindingContextPredicate, Unbind, VisualTestContext};
        cx.update(super::init);
        let cx = cx.add_empty_window();
        let tooltip = |cx: &mut VisualTestContext| {
            cx.update(|window, _| super::toolbar::annotate_tooltip(window))
        };
        // The user's binding of `keys` to `action` in the browser, as the keymap file binds it.
        let bind = |keys: &str, action: Box<dyn Action>, cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                let browser = KeyBindingContextPredicate::parse("Browser").ok();
                let context = browser.map(std::rc::Rc::new);
                let binding = KeyBinding::load(keys, action, context, false, None, cx).unwrap();
                cx.bind_keys([binding.with_meta(crate::keymap::USER)]);
            })
        };
        let off = || Box::new(Unbind("browser::ToggleAnnotate".into()));
        assert_eq!(tooltip(cx).as_ref(), "Annotate (⇧⌘.)");

        bind("cmd->", off(), cx);
        bind("ctrl-alt-a", Box::new(super::ToggleAnnotate), cx);
        assert_eq!(tooltip(cx).as_ref(), "Annotate (⌃⌥A)");

        bind("ctrl-alt-a", off(), cx);
        assert_eq!(tooltip(cx).as_ref(), "Annotate");
    }

    /// ⇧⌘. toggles Annotate while Sawit's side of the tab has the keyboard: the Annotate and
    /// Design buttons and a click in the side panel give it to the view, and every pick to the
    /// new note's text box. A Mac reports ⇧⌘. as `cmd->` (gpui's macOS `parse_keystroke` turns
    /// Shift off for a shifted symbol), so that is the key pressed here, in the workspace with
    /// every key the workspace and the text boxes bind.
    #[gpui_kit::test]
    fn shift_cmd_period_toggles_annotate_from_sawits_side(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::component::input::{Input, InputState};
        use gpui_kit::prelude::*;
        use gpui_kit::{Entity, FocusHandle, Focusable as _, VisualTestContext, Window, div};

        /// The browser tab's key context in the workspace's, with a note's text box, counting
        /// the toggles it gets.
        struct BrowserSide {
            focus: FocusHandle,
            note: Entity<InputState>,
            toggles: usize,
        }
        impl Render for BrowserSide {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let browser = div()
                    .key_context("Browser")
                    .track_focus(&self.focus)
                    .on_action(cx.listener(|this, _: &super::ToggleAnnotate, _, _| {
                        this.toggles += 1;
                    }))
                    .size_full()
                    .child(Input::new(&self.note));
                div().key_context("Workspace").size_full().child(browser)
            }
        }

        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::workspace::init(cx);
        });
        let (view, cx) = cx.add_window_view(|window, cx| BrowserSide {
            focus: cx.focus_handle(),
            note: cx.new(|cx| InputState::new(window, cx)),
            toggles: 0,
        });
        let toggles = |cx: &mut VisualTestContext| cx.update(|_, cx| view.read(cx).toggles);

        // The view has the keyboard (the toolbar's buttons, a click in the panel).
        cx.update(|window, cx| {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("cmd->");
        assert_eq!(toggles(cx), 1, "⇧⌘. as a Mac sends it toggles Annotate");
        cx.simulate_keystrokes("cmd->");
        assert_eq!(toggles(cx), 2, "and toggles it back");

        // A note's text box has it (after a pick).
        let note = cx.update(|_, cx| view.read(cx).note.clone());
        cx.update(|window, cx| note.update(cx, |note, cx| note.focus(window, cx)));
        assert!(cx.update(|window, cx| note.read(cx).focus_handle(cx).is_focused(window)));
        cx.simulate_keystrokes("cmd->");
        assert_eq!(toggles(cx), 3, "from the note's text box too");
    }

    /// A browser tab's view in a window, not drawn (its WKWebView needs a real window), shown as
    /// the workspace shows it.
    fn browser_view(cx: &mut gpui_kit::VisualTestContext) -> gpui_kit::Entity<super::BrowserView> {
        use gpui_kit::AppContext as _;
        cx.update(|window, cx| {
            let view = cx.new(|cx| super::BrowserView::new(None, None, window, cx));
            view.update(cx, |view, _| view.visible = true);
            view
        })
    }

    /// Enter in the address field loads the page and gives it the keyboard: AppKit's, and
    /// GPUI's, which goes to the browser's own view, so that the ⌘ keys GPUI sees first (⌘V, ⌘A,
    /// ⌘Z) no longer act on the address field while the page has the keyboard.
    #[gpui_kit::test]
    fn enter_in_the_address_gives_the_page_the_keyboard(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::component::input::InputEvent;
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let cx = cx.add_empty_window();
        let view = browser_view(cx);
        let address = cx.update(|_, cx| view.read(cx).address.clone());
        cx.update(|window, cx| {
            address.update(cx, |a, cx| {
                a.set_value("localhost:3000", window, cx);
                a.focus(window, cx);
            })
        });
        cx.update(|_, cx| {
            address.update(cx, |_, cx| {
                cx.emit(InputEvent::PressEnter {
                    secondary: false,
                    shift: false,
                })
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(view.read(cx).focus.is_focused(window));
            assert_eq!(view.read(cx).url, "http://localhost:3000");
        });
    }

    /// ⌘C goes to the page while the browser's view has GPUI's keyboard: no binding of Sawit's
    /// takes it (gpui-kit's root copies its own text selection on ⌘C).
    #[gpui_kit::test]
    fn cmd_c_in_the_browser_is_left_to_the_page(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::{KeyContext, Keystroke, OperatingSystem};
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::workspace::init(cx);
        });
        cx.update(|cx| {
            let typed = [Keystroke::parse_with_os("cmd-c", OperatingSystem::Mac).unwrap()];
            let stack = ["Root", "Workspace", "Browser"].map(|c| KeyContext::parse(c).unwrap());
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let (bindings, _) = keymap.bindings_for_input(&typed, &stack);
            let names: Vec<&str> = bindings.iter().map(|b| b.action().name()).collect();
            assert!(names.is_empty(), "{names:?}");
        });
    }

    /// The user's press in the page (the page agent's `pressed` message, sent for a real press
    /// only) moves GPUI's keyboard to the browser's view, as AppKit gave the page its own: off
    /// the address field, the terminal or Files, whose ⌘V, ⌘C and ⌘Z would otherwise act there.
    #[gpui_kit::test]
    fn a_press_in_the_page_moves_gpuis_keyboard_to_it(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let cx = cx.add_empty_window();
        let view = browser_view(cx);
        let address = cx.update(|_, cx| view.read(cx).address.clone());
        cx.update(|window, cx| address.update(cx, |a, cx| a.focus(window, cx)));
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.receive(
                    super::FromPage::Event(sawit_core::browser::PageEvent::Pressed),
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| assert!(view.read(cx).focus.is_focused(window)));
    }

    /// A press reported while the tab is hidden leaves GPUI's keyboard where the user has it (the
    /// terminal, say): the user can't press in a hidden page, and a view that isn't drawn must
    /// not take the keyboard: keys would go from the window's root, so that ⌘W, ⌘T and ⌘1 stop
    /// working, until the workspace takes it back (`workspace::KeyboardHome`), and
    /// then the terminal no longer has it.
    #[gpui_kit::test]
    fn a_press_in_a_hidden_page_leaves_the_keyboard(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let cx = cx.add_empty_window();
        let view = browser_view(cx);
        let elsewhere = cx.update(|_, cx| cx.focus_handle());
        cx.update(|window, cx| {
            view.update(cx, |view, _| view.visible = false);
            elsewhere.focus(window, cx);
        });
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.receive(
                    super::FromPage::Event(sawit_core::browser::PageEvent::Pressed),
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| {
            assert!(
                elsewhere.is_focused(window),
                "a hidden page's press took the keyboard: the view has it = {}",
                view.read(cx).focus.is_focused(window)
            )
        });
    }

    /// Where the keyboard is when the browser's tab goes off screen.
    #[derive(Clone, Copy, Debug)]
    enum Keyboard {
        /// The workspace: a click in the page never moved GPUI's keyboard before KEYMAP-4.
        Workspace,
        /// The browser's view, after the user's press in the page.
        Page,
        /// The address field, while the user types an address.
        Address,
    }

    /// I3: the browser's tab often goes off screen without a click, after a press in the page
    /// gave its view GPUI's keyboard (KEYMAP-4): ⇧⌘J hides the tab area, a page comes in front
    /// without taking the keyboard (an agent's `app_open_page`), ⌃Tab, an agent's
    /// `app_activate_tab`. A view that isn't drawn and still has the keyboard makes GPUI send
    /// the keys from the window's root, gpui-kit's `Root`, past every workspace shortcut, until a
    /// click: ⇧⌘J didn't bring the tab area back, ⌘W didn't close the page in front. The
    /// workspace takes the keyboard back ([`crate::workspace::KeyboardHome`]), from the page and
    /// the address field alike.
    ///
    /// The workspace here is a stand-in, in a window as `cli::open_window` makes one (under
    /// gpui-kit's `Root`): the workspace's keys and key context, its keyboard, `render`'s refocus
    /// when nothing has the keyboard, its `KeyboardHome`, given back to as `render` does (never to
    /// the browser's view), and the browser's tab, told whether it shows as `sync_browser` tells
    /// it and drawn while it shows (its key context, its view's keyboard, the address field). The
    /// webview itself needs a real window.
    #[gpui_kit::test]
    fn the_workspace_keys_work_once_the_browser_tab_is_hidden(cx: &mut gpui_kit::TestAppContext) {
        use super::{BrowserView, FromPage};
        use crate::workspace::{CloseActiveTab, OpenMcp, ToggleCenter};
        use gpui_kit::component::input::Input;
        use gpui_kit::prelude::*;
        use gpui_kit::{
            AnyWindowHandle, Entity, FocusHandle, Focusable as _, TestAppContext, Window, div,
        };
        use sawit_core::browser::PageEvent;

        struct StandIn {
            focus: FocusHandle,
            browser: Entity<BrowserView>,
            /// The tab area shows: ⇧⌘J toggles it.
            area: bool,
            /// Another tab is in front of the browser's, and takes no keyboard, as a page an agent
            /// opens: ⌃⌘M opens it here, ⌘W closes it.
            other_tab: bool,
            toggles: usize,
            closes: usize,
            keyboard: crate::workspace::KeyboardHome<bool>,
        }
        impl Render for StandIn {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                if window.focused(cx).is_none() {
                    self.focus.focus(window, cx);
                }
                let shown = self.area && !self.other_tab;
                self.browser.update(cx, |b, _| b.set_visible(shown));
                let (view, address) = {
                    let browser = self.browser.read(cx);
                    (browser.focus.clone(), browser.address.clone())
                };
                self.keyboard
                    .give_back(shown, |back, _| *back != view, window, cx);
                div()
                    .key_context("Workspace")
                    .track_focus(&self.focus)
                    .size_full()
                    .on_action(cx.listener(|this, _: &ToggleCenter, _, cx| {
                        this.area = !this.area;
                        this.toggles += 1;
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &OpenMcp, _, cx| {
                        this.other_tab = true;
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &CloseActiveTab, _, cx| {
                        this.other_tab = false;
                        this.closes += 1;
                        cx.notify();
                    }))
                    .when(shown, |el| {
                        el.child(
                            div()
                                .key_context("Browser")
                                .track_focus(&view)
                                .size_full()
                                .child(Input::new(&address)),
                        )
                    })
            }
        }

        /// A window with the stand-in, and the keyboard put where `keyboard` says.
        fn open(keyboard: Keyboard, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<StandIn>) {
            let (window, workspace) = cx.update(|cx| {
                gpui_kit::open_window(Default::default(), cx, |window, cx| {
                    let browser = cx.new(|cx| BrowserView::new(None, None, window, cx));
                    cx.new(|cx| {
                        let focus = cx.focus_handle();
                        StandIn {
                            keyboard: crate::workspace::KeyboardHome::new(
                                focus.clone(),
                                window,
                                cx,
                            ),
                            focus,
                            browser,
                            area: true,
                            other_tab: false,
                            toggles: 0,
                            closes: 0,
                        }
                    })
                })
                .unwrap()
            });
            cx.update_window(window, |_, window, _| window.activate_window())
                .unwrap();
            cx.run_until_parked();
            let browser = cx.update(|cx| workspace.read(cx).browser.clone());
            let address = cx.update(|cx| browser.read(cx).address.clone());
            cx.update_window(window, |_, window, cx| match keyboard {
                Keyboard::Workspace => {}
                Keyboard::Page => browser.update(cx, |b, cx| {
                    b.receive(FromPage::Event(PageEvent::Pressed), window, cx)
                }),
                Keyboard::Address => address.update(cx, |a, cx| a.focus(window, cx)),
            })
            .unwrap();
            cx.run_until_parked();
            let placed = cx
                .update_window(window, |_, window, cx| {
                    let has = |focus: &FocusHandle| focus.is_focused(window);
                    match keyboard {
                        Keyboard::Workspace => has(&workspace.read(cx).focus),
                        Keyboard::Page => has(&browser.read(cx).focus),
                        Keyboard::Address => has(&address.read(cx).focus_handle(cx)),
                    }
                })
                .unwrap();
            assert!(placed, "the {keyboard:?} doesn't have the keyboard");
            (window, workspace)
        }
        fn press(keys: &str, window: AnyWindowHandle, cx: &mut TestAppContext) {
            cx.simulate_keystrokes(window, keys);
            cx.run_until_parked();
        }

        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::workspace::init(cx);
        });
        let mut failed = Vec::new();
        for keyboard in [Keyboard::Workspace, Keyboard::Page, Keyboard::Address] {
            // ⇧⌘J three times: the tab area hides, shows and hides again. The keyboard is then
            // the workspace's, not the hidden browser's.
            let (window, workspace) = open(keyboard, cx);
            for _ in 0..3 {
                press("cmd-shift-j", window, cx);
            }
            let (toggles, area) =
                cx.update(|cx| (workspace.read(cx).toggles, workspace.read(cx).area));
            if (toggles, area) != (3, false) {
                failed.push(format!(
                    "{keyboard:?}: ⇧⌘J pressed 3 times toggled the tab area {toggles} times"
                ));
            }
            let (workspace_has_it, browser_has_it) = cx
                .update_window(window, |_, window, cx| {
                    let browser = workspace.read(cx).browser.read(cx);
                    (
                        workspace.read(cx).focus.is_focused(window),
                        browser.focus.contains_focused(window, cx)
                            || browser.address.read(cx).focus_handle(cx).is_focused(window),
                    )
                })
                .unwrap();
            if !workspace_has_it || browser_has_it {
                failed.push(format!(
                    "{keyboard:?}: the workspace has the keyboard = {workspace_has_it}, \
                     the hidden browser = {browser_has_it}"
                ));
            }

            // A page that takes no keyboard comes in front of the browser (⌃⌘M here, as an agent's
            // `app_open_page`), and ⌘W closes it.
            let (window, workspace) = open(keyboard, cx);
            press("ctrl-cmd-m", window, cx);
            press("cmd-w", window, cx);
            let (closes, other_tab) =
                cx.update(|cx| (workspace.read(cx).closes, workspace.read(cx).other_tab));
            if (closes, other_tab) != (1, false) {
                failed.push(format!(
                    "{keyboard:?}: ⌘W after ⌃⌘M closed {closes} pages, \
                     the MCP page still in front = {other_tab}"
                ));
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }
}
