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

mod agents;
mod annotate;
mod capture;
mod cover;
mod design;
mod device;
pub(crate) mod engine;
mod events;
mod favicon;
mod mesurer;
mod modes;
mod navigation;
mod notes;
mod ops;
mod panel;
mod picking;
mod presence;
mod progress;
mod seo;
mod servers;
mod shots;
mod suspend;
pub mod tabs;
mod toolbar;
mod viewport;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::browser::{BrowserOp, Logs, PageEvent, ViewportMode};
use sawit_core::probe;
use sawit_core::terminal::AgentKind;
use serde_json::{Value, json};

pub use agents::{AgentSource, installed_agents, open_command, write_prompt};
pub use annotate::notes_lost;
use annotate::{AnnotateEvent, AnnotatePanel};
use design::{DesignEvent, DesignPanel};
use engine::{Engine, EngineEvent};
pub use modes::{Panel, PickMode};
use navigation::ExternalGate;
pub use navigation::address_url;
use seo::{SeoEvent, SeoPanel};
use shots::ShotGate;
pub use tabs::{BrowserTabs, TabsEvent};
use viewport::{CAPTION_BAND, caption};

use crate::rt;
use crate::states::Placeholder;
use crate::terminal::RunningAgent;

/// The tab's icon and title.
pub const ICON: Lucide = Lucide::Globe;
pub const TITLE: &str = "Browser";

actions!(browser, [ToggleAnnotate]);
// mesurer's shortcuts, while it is on the page (`mesurer/keys.rs`).
actions!(
    browser,
    [
        CopyCommentsForAgent,
        ResolveAllComments,
        SelectAllAnnotations
    ]
);

/// The Annotate panel's Show tools: mesurer, minimized, shows its toolbar again.
pub struct ShowMesurerTools;

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
    cx.bind_keys(BrowserView::mesurer_bindings());
    tabs::init(cx);
}

const TOOLBAR_HEIGHT: f32 = 36.;

type OpResult = Result<Value, String>;

/// What the engine reports, from its handlers (any thread) to the view.
enum FromPage {
    Event(PageEvent),
    Engine(EngineEvent),
    /// The page's icon `<link>` tags, as JSON ([`favicon`]).
    IconLinks(String),
    /// An element's box is known: capture it (or the whole view).
    Shoot {
        rect: Option<(f64, f64, f64, f64)>,
        reply: oneshot::Sender<OpResult>,
    },
}

/// What the browser asks of the workspace, which answers with [`BrowserView::finish_send`].
pub enum BrowserEvent {
    /// Send the notes, as Markdown, to this agent's chat.
    SendToAgent {
        markdown: String,
        agent: RunningAgent,
    },
    /// Start this agent in a new terminal with the notes as its first message.
    OpenAgent { markdown: String, kind: AgentKind },
    /// Open `url` in a new tab behind this one (a ⌘-click on a local server); the tabs hear it.
    OpenTab { url: String },
}

impl EventEmitter<BrowserEvent> for BrowserView {}

pub struct BrowserView {
    project: Option<PathBuf>,
    engine: Option<Rc<Engine>>,
    find: events::find::BrowserFind, // ⌘F's bar above the page
    error: Option<SharedString>,
    bounds: Option<Bounds<Pixels>>,
    /// The tab is active and the center pane is showing.
    visible: bool,
    /// The device menu is open, over the page: the webview is hidden meanwhile
    /// ([`Self::set_menu_open`]).
    menu_open: bool,
    /// Whether a menu, popover, dialog or notification covers the page, and the page's picture
    /// in its place meanwhile ([`cover`]).
    cover: crate::native_cover::Cover,
    frozen: cover::Frozen,
    address: Entity<InputState>,
    url: String,
    /// WebKit's address as the toolbar was last drawn with it (the Explorer button and Open in
    /// Default Browser follow it); none before the first frame.
    drawn_url: Option<String>,
    title: String,
    /// The page's favicon on its tab ([`favicon`]).
    icon: favicon::TabIcon,
    loading: bool,
    /// How far the page in [`Self::loading`] has come, while it loads ([`progress`]).
    progress: progress::LoadProgress,
    /// The local servers' list, while it is open ([`servers`]).
    servers: Option<servers::Scan>,
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
    /// The SEO panel ([`seo`]), the scan it last asked for, and a count that tells a scan's
    /// answer from an older one's.
    seo_panel: Entity<SeoPanel>,
    seo_ask: seo::flow::SeoAsk,
    seo_generation: u64,
    /// The recording under way, and how the last one ended ([`capture::record`]).
    recorder: capture::record::Recorder,
    /// The capture menu's settings and the capture bar ([`capture::ui`]).
    capture_ui: capture::ui::CaptureUi,
    /// Shots take turns: the overlay has one hidden flag.
    shots: ShotGate,
    pick: PickMode,
    /// Picking was asked for but the page had no agent yet; it is armed
    /// again when the page has loaded.
    pick_waiting: bool,
    to_view: UnboundedSender<FromPage>,
    /// The design scan (cached per page), inspect and export.
    design: design::flow::DesignFlow,
    /// The page given up off screen, and what brings it back ([`suspend`]).
    suspended: suspend::Suspended,
    /// Ops sent before the webview exists, in order.
    queued: Vec<(BrowserOp, oneshot::Sender<OpResult>)>,
    /// Links to other apps the page opened: asked about one at a time.
    external: ExternalGate,
    /// The agents at work in the browser ([`presence`]).
    agents: presence::Agents,
    /// mesurer, Annotate's tools on the page ([`mesurer`]).
    mesurer: mesurer::MesurerState,
    _ipc: Task<()>,
    /// Hands the Mac's font changes to the Design panel ([`engine::font_changes`]).
    _fonts: Task<()>,
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
        let seo_panel = cx.new(|_| SeoPanel::new());
        cx.default_global::<crate::local_explorer::Running>();
        let mut subscriptions = vec![
            // The Explorer button comes and goes with the page's dev server's Local Explorer.
            cx.observe_global::<crate::local_explorer::Running>(|_, cx| cx.notify()),
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
                &seo_panel,
                window,
                |this, _, event: &SeoEvent, _, cx| match event {
                    SeoEvent::Rescan => this.seo_rescan(cx),
                    SeoEvent::ScanType(t) => {
                        this.seo_ask.scan_type = *t;
                        this.seo_rescan(cx);
                    }
                    SeoEvent::External(on) => {
                        this.seo_ask.external = *on;
                        this.seo_panel.update(cx, |p, cx| p.set_external(*on, cx));
                        this.seo_rescan(cx);
                    }
                },
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
            // Agents' cursors turned off: the page's go.
            cx.observe_global::<crate::agents::Agents>(|this, cx| this.agents_setting_changed(cx)),
        ];
        subscriptions.extend(Self::mesurer_subscriptions(&annotate, cx));
        let (to_view, mut from_page) = unbounded();
        let ipc = cx.spawn_in(window, async move |this, cx| {
            while let Some(message) = from_page.next().await {
                let alive = this.update_in(cx, |this, window, cx| {
                    this.receive(message, window, cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        // Fonts installed or removed: the Design panel reads the Mac's families again.
        let mut font_changes = engine::font_changes();
        let fonts = cx.spawn(async move |this, cx| {
            while font_changes.next().await.is_some() {
                let alive = this.update(cx, |this, cx| {
                    (this.design_panel).update(cx, |panel, cx| panel.fonts_changed(cx))
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
            find: Default::default(),
            error: None,
            bounds: None,
            visible: false,
            menu_open: false,
            cover: Default::default(),
            frozen: Default::default(),
            address,
            url: url.unwrap_or_else(|| "about:blank".into()),
            drawn_url: None,
            title: String::new(),
            icon: Default::default(),
            loading: false,
            progress: Default::default(),
            servers: None,
            navigated,
            token: navigation::new_token(),
            logs: Logs::default(),
            last_pick: None,
            viewport: ViewportMode::Responsive,
            focus: cx.focus_handle(),
            panel: None,
            annotate,
            design_panel,
            seo_panel,
            seo_ask: Default::default(),
            seo_generation: 0,
            recorder: Default::default(),
            capture_ui: Default::default(),
            shots: ShotGate::default(),
            pick: PickMode::Off,
            pick_waiting: false,
            to_view,
            design: Default::default(),
            queued: Vec::new(),
            suspended: Default::default(),
            external: ExternalGate::default(),
            agents: presence::Agents::new(crate::agents::shown(cx)),
            mesurer: Default::default(),
            _ipc: ipc,
            _fonts: fonts,
            _subscriptions: subscriptions,
        }
    }

    /// The page as `app_state` shows it.
    pub fn describe(&self) -> Value {
        json!({
            "url": self.current_url(),
            "title": self.title,
            // Whether the tab shows the page's favicon ([`favicon`]).
            "favicon": self.icon.image.is_some(),
            "loading": self.loading,
            "progress": self.loading.then_some(self.progress.value),
            "viewport": self.viewport_json(),
            "panel": self.panel.map(Panel::as_str),
            "pick_mode": self.pick.as_str(),
            "last_pick": self.last_pick,
            "mesurer": self.mesurer_describe(),
            "recording": self.record_status(),
            // Whether a menu, popover, dialog or notification covers the page, and whether the
            // webview is up (it steps back while something covers it).
            "covered": self.cover.covered,
            "page_shown": self.page_shown(),
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
        let url = self.current_url();
        self.drawn_url = Some(url.clone());
        let this = cx.entity().downgrade();
        let device = match &self.viewport {
            ViewportMode::Fixed {
                name,
                width,
                height,
            } => Some(caption(name.as_deref(), *width, *height, self.scale())),
            ViewportMode::Responsive => None,
        };
        let frozen = self.frozen_paint();
        let page = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            // The page hidden under the device menu leaves the same backdrop as a device's.
            .when(
                device.is_some() || self.menu_open || self.frozen.hidden,
                |d| d.bg(cx.theme().muted),
            )
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
                    move |bounds, _, window, _| frozen.paint(bounds, window),
                )
                .size_full(),
            )
            .child(Self::cover_watch(cx));
        let side = self.panel.map(|panel| self.side_panel(panel, cx));
        v_flex()
            .key_context(self.mesurer_key_context(window))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleAnnotate, _, cx| {
                this.annotate_key(cx);
            }))
            .map(|el| Self::with_mesurer_keys(el, cx))
            .size_full()
            .child(self.toolbar(&url, window, cx))
            .children(self.capture_bar(cx))
            .map(|el| self.with_find(el, cx))
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
