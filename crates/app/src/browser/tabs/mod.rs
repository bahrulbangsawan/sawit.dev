//! The browser's tabs: one [`BrowserView`] each, in a strip above the toolbar ([`strip`]). Each
//! keeps its own address, history, device size, notes and panels. Only the tab on show keeps its
//! web view for long: the others give theirs up after [`SUSPEND_AFTER`] off screen
//! ([`super::suspend`]), so memory doesn't grow with the tab count, and get it back when they
//! show, or off screen when an agent acts on them ([`BrowserTabs::wake`]). The tabs are kept
//! between runs ([`store`]).
//!
//! ⌘T opens a tab, ⌘W closes the one on show and ⌘1…⌘8 show a tab by its place (⌘9 the last),
//! while the browser has the keyboard (the `Browser` key context): elsewhere those keys are the
//! workspace's.

mod store;
mod strip;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::BrowserView;

actions!(
    browser_tabs,
    [
        NewTab, CloseTab, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, LastTab
    ]
);

/// How long a tab off screen keeps its web view.
pub const SUSPEND_AFTER: Duration = Duration::from_secs(60);
/// How often the tabs off screen are looked at.
const SWEEP: Duration = Duration::from_secs(10);

pub fn init(cx: &mut App) {
    let ctx = Some("Browser");
    cx.bind_keys([
        // B5: ⌘T opens and closes the terminal everywhere, the browser too; a new tab is ⌃T.
        KeyBinding::new("ctrl-t", NewTab, ctx),
        KeyBinding::new("cmd-w", CloseTab, ctx),
        KeyBinding::new("cmd-1", Tab1, ctx),
        KeyBinding::new("cmd-2", Tab2, ctx),
        KeyBinding::new("cmd-3", Tab3, ctx),
        KeyBinding::new("cmd-4", Tab4, ctx),
        KeyBinding::new("cmd-5", Tab5, ctx),
        KeyBinding::new("cmd-6", Tab6, ctx),
        KeyBinding::new("cmd-7", Tab7, ctx),
        KeyBinding::new("cmd-8", Tab8, ctx),
        KeyBinding::new("cmd-9", LastTab, ctx),
    ]);
}

/// What the tabs tell the workspace.
pub enum TabsEvent {
    /// A tab was made: the workspace hears its notes' sends from it.
    Added(Entity<BrowserView>),
    /// The last tab closed: the browser's own tab goes.
    Emptied,
}

impl EventEmitter<TabsEvent> for BrowserTabs {}

struct Tab {
    id: u64,
    view: Entity<BrowserView>,
    /// Last on show, or acted on by an agent.
    used: Instant,
    /// The address, title and device size last written.
    seen: String,
    _observe: [Subscription; 2],
}

pub struct BrowserTabs {
    project: Option<PathBuf>,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    /// The browser's own tab is the one on show.
    on_show: bool,
    /// A save is waiting ([`store`]).
    saving: bool,
    _sweep: Task<()>,
}

impl BrowserTabs {
    /// The browser of `project`, with the tabs it had last time; one tab at `url` (or the
    /// project's dev server) when it had none. With `url`, the tab on show opens it.
    pub fn new(
        project: Option<PathBuf>,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sweep = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SWEEP).await;
                if this.update(cx, |this, cx| this.sweep(cx)).is_err() {
                    break;
                }
            }
        });
        let mut this = BrowserTabs {
            project: project.clone(),
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            on_show: false,
            saving: false,
            _sweep: sweep,
        };
        let saved = store::load(project.as_deref());
        for tab in &saved.tabs {
            let view = this.push(Some(tab.url.clone()), window, cx);
            let history = tab
                .history
                .as_deref()
                .and_then(sawit_core::browser::base64_decode);
            let viewport = tab.viewport.clone();
            let url = tab.url.clone();
            view.update(cx, |v, cx| {
                v.restore(url, history, cx);
                v.set_viewport(viewport, cx);
            });
        }
        this.active = saved.active.min(this.tabs.len().saturating_sub(1));
        match (this.tabs.is_empty(), url) {
            (true, url) => {
                this.push(url, window, cx);
            }
            (false, Some(url)) => {
                let view = this.active_view();
                view.update(cx, |v, cx| v.show(Some(&url), cx));
            }
            (false, None) => {}
        }
        this
    }

    /// Makes a tab at the end, at `url` (none: the project's dev server, else a blank page).
    fn push(
        &mut self,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        let project = self.project.clone();
        let view = cx.new(|cx| BrowserView::new(project, url, window, cx));
        // A ⌘-click on a local server opens a tab behind this one.
        let opens = cx.subscribe_in(&view, window, |this, _, event, window, cx| {
            if let super::BrowserEvent::OpenTab { url } = event {
                this.open_tab(Some(url.clone()), false, window, cx);
            }
        });
        // Written again when a tab's address, title or device size changes, not on each redraw.
        let observe = cx.observe(&view, |this, view, cx| {
            let v = view.read(cx);
            let now = format!("{}\n{}\n{:?}", v.current_url(), v.title, v.viewport);
            if let Some(tab) = this.tabs.iter_mut().find(|t| t.view == view)
                && tab.seen != now
            {
                tab.seen = now;
                this.save_soon(cx);
            }
            cx.notify();
        });
        self.tabs.push(Tab {
            id: self.next_id,
            view: view.clone(),
            used: Instant::now(),
            seen: String::new(),
            _observe: [observe, opens],
        });
        self.next_id += 1;
        cx.emit(TabsEvent::Added(view.clone()));
        view
    }

    /// Opens a tab at `url` after the others; shown when `show` (the user's ⌘T or +), else behind
    /// the one on show (an agent's, or a ⌘-click on a local server).
    pub fn open_tab(
        &mut self,
        url: Option<String>,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        let view = self.push(url, window, cx);
        if show {
            self.activate(self.tabs.len() - 1, cx);
        }
        self.save_soon(cx);
        cx.notify();
        view
    }

    /// The tab on show.
    pub fn active_view(&self) -> Entity<BrowserView> {
        self.tabs[self.active].view.clone()
    }

    /// Every tab, in order.
    pub fn views(&self) -> Vec<Entity<BrowserView>> {
        self.tabs.iter().map(|t| t.view.clone()).collect()
    }

    /// The tab an agent names by id, title or address ([`sawit_core::browser::find_tab`]).
    pub fn find(&self, name: &str, cx: &App) -> Option<(usize, Entity<BrowserView>)> {
        let described: Vec<(u64, String, String)> = self
            .tabs
            .iter()
            .map(|t| {
                let v = t.view.read(cx);
                (t.id, v.title.clone(), v.current_url())
            })
            .collect();
        let rows: Vec<(u64, &str, &str)> = described
            .iter()
            .map(|(id, title, url)| (*id, title.as_str(), url.as_str()))
            .collect();
        let ix = sawit_core::browser::find_tab(&rows, name)?;
        Some((ix, self.tabs[ix].view.clone()))
    }

    /// Shows tab `ix`; the one that was on show hides, its keyboard given back.
    pub fn activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() || ix == self.active {
            return;
        }
        let now = Instant::now();
        self.tabs[self.active].used = now;
        self.tabs[ix].used = now;
        self.active = ix;
        self.sync(cx);
        self.save_soon(cx);
        cx.notify();
    }

    /// Closes tab `ix`, asking first when its notes weren't copied or sent: they live only in the
    /// tab. The last tab closing closes the browser ([`TabsEvent::Emptied`]).
    pub fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        let notes = tab.view.read(cx).unsent_notes(cx);
        if notes == 0 {
            self.remove(ix, cx);
            return;
        }
        let view = tab.view.clone();
        let answer = crate::confirm::Confirm::new(
            "Close the tab with its annotations?",
            "Close and Discard Notes",
        )
        .detail(super::notes_lost(notes))
        .ask(window, cx);
        cx.spawn(async move |this, cx| {
            if answer.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this.tabs.iter().position(|t| t.view == view) {
                        this.remove(ix, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        let tab = self.tabs.remove(ix);
        tab.view.update(cx, |v, _| v.set_visible(false));
        if self.tabs.is_empty() {
            self.active = 0;
            store::forget(self.project.as_deref());
            cx.emit(TabsEvent::Emptied);
            return;
        }
        if ix < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
        self.sync(cx);
        self.save_soon(cx);
        cx.notify();
    }

    /// Moves tab `from` to place `to` (a drag in the strip).
    pub fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let active = self.tabs[self.active].id;
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = self.tabs.iter().position(|t| t.id == active).unwrap_or(0);
        self.save_soon(cx);
        cx.notify();
    }

    /// The browser's own tab is on show (`on_show`), or not: only the tab on show of a browser
    /// on show shows its page.
    pub fn set_on_show(&mut self, on_show: bool, cx: &mut Context<Self>) {
        self.on_show = on_show;
        self.sync(cx);
    }

    /// Hides every tab but the one on show (hidden first: a page that hides gives the keyboard
    /// back), and shows that one while the browser is on show.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let active = self.active;
        for (ix, tab) in self.tabs.iter().enumerate() {
            if ix != active {
                tab.view.update(cx, |v, _| v.set_visible(false));
            }
        }
        let on_show = self.on_show;
        if let Some(tab) = self.tabs.get_mut(active) {
            // On show it stays live; off screen (another tab of the window in front) its time
            // runs as the others' does.
            if on_show {
                tab.used = Instant::now();
            }
            tab.view.update(cx, |v, _| v.set_visible(on_show));
        }
    }

    /// Wakes tab `ix` for an agent's op: its web view is built off screen if it gave it up, at
    /// the page area of the tab on show. The tab on show and the keyboard stay as they are.
    pub fn wake(&mut self, ix: usize, window: &Window, cx: &mut Context<Self>) {
        let bounds = self.tabs[self.active].view.read(cx).page_bounds();
        if let Some(tab) = self.tabs.get_mut(ix) {
            tab.used = Instant::now();
            tab.view.update(cx, |v, cx| v.wake(bounds, window, cx));
        }
    }

    /// Gives up the web views of tabs off screen for [`SUSPEND_AFTER`].
    fn sweep(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        for (ix, tab) in self.tabs.iter().enumerate() {
            let shown = self.on_show && ix == self.active;
            if shown || now.duration_since(tab.used) < SUSPEND_AFTER {
                continue;
            }
            tab.view.update(cx, |v, cx| v.suspend(cx));
        }
    }

    /// The tabs as `app_state` and `browser_tabs` list them.
    pub fn describe(&self, cx: &App) -> serde_json::Value {
        let tabs: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, t)| {
                let v = t.view.read(cx);
                serde_json::json!({
                    "id": t.id,
                    "title": v.title,
                    "url": v.current_url(),
                    "active": ix == self.active,
                    "live": v.is_live(),
                    "loading": v.loading,
                    "device": v.viewport_json()["device"],
                })
            })
            .collect();
        serde_json::json!(tabs)
    }

    /// The id of tab `ix`.
    pub fn id_of(&self, ix: usize) -> Option<u64> {
        self.tabs.get(ix).map(|t| t.id)
    }

    /// How many tabs there are, and which is on show.
    pub fn counts(&self) -> (usize, usize) {
        (self.tabs.len(), self.active)
    }
}

impl Focusable for BrowserTabs {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.tabs[self.active].view.read(cx).focus_handle(cx)
    }
}

impl Render for BrowserTabs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.active_view();
        v_flex()
            .size_full()
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                let view = this.open_tab(None, true, window, cx);
                view.update(cx, |v, cx| v.focus_address(window, cx));
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.close_tab(this.active, window, cx);
            }))
            .on_action(cx.listener(|this, _: &Tab1, _, cx| this.activate(0, cx)))
            .on_action(cx.listener(|this, _: &Tab2, _, cx| this.activate(1, cx)))
            .on_action(cx.listener(|this, _: &Tab3, _, cx| this.activate(2, cx)))
            .on_action(cx.listener(|this, _: &Tab4, _, cx| this.activate(3, cx)))
            .on_action(cx.listener(|this, _: &Tab5, _, cx| this.activate(4, cx)))
            .on_action(cx.listener(|this, _: &Tab6, _, cx| this.activate(5, cx)))
            .on_action(cx.listener(|this, _: &Tab7, _, cx| this.activate(6, cx)))
            .on_action(cx.listener(|this, _: &Tab8, _, cx| this.activate(7, cx)))
            .on_action(
                cx.listener(|this, _: &LastTab, _, cx| this.activate(this.tabs.len() - 1, cx)),
            )
            .child(self.strip(window, cx))
            .child(div().flex_1().min_h_0().child(view))
    }
}
