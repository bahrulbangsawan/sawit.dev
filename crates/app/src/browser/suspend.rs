//! A tab off screen gives its web view up (the WKWebView and its web content process) after a
//! while, so memory doesn't grow with the tab count: it keeps its address, its history
//! ([`Engine::history`](super::engine::Engine::history)), its device size, its notes and its
//! panels, and builds the view again when it shows, or off screen when an agent acts on it
//! ([`BrowserView::wake`]). A view woken off screen is hidden, never key and never the keyboard's
//! host: the keyboard stays where the user has it ([`super::engine::keyboard`]).

use gpui_kit::*;

use super::BrowserView;

/// What a tab that gave its view up keeps of it.
#[derive(Default)]
pub(super) struct Suspended {
    /// The page's history, put back when the view is built again.
    pub history: Option<Vec<u8>>,
    /// The view has the keyboard's host: set once the tab is on show.
    pub keyboard_host: bool,
}

impl BrowserView {
    /// Whether the tab has its web view now.
    pub fn is_live(&self) -> bool {
        self.engine.is_some()
    }

    /// Whether the tab is the one on show (its pane on screen).
    pub fn on_show(&self) -> bool {
        self.visible
    }

    /// The page's history: the live view's, else what it kept when it gave the view up.
    pub fn history(&self) -> Option<Vec<u8>> {
        match &self.engine {
            Some(engine) => engine.history(),
            None => self.suspended.history.clone(),
        }
    }

    /// Starts from `url` with `history` (a restored tab): the view is built with them when the
    /// tab first shows or an agent wakes it.
    pub fn restore(&mut self, url: String, history: Option<Vec<u8>>, cx: &mut Context<Self>) {
        self.navigated = true;
        self.url = url;
        self.suspended.history = history;
        cx.notify();
    }

    /// Gives the web view up, keeping the address and history to build it again. Not while the
    /// tab is on show, nor with ops waiting on it.
    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        if self.visible || !self.queued.is_empty() {
            return;
        }
        let Some(engine) = self.engine.take() else {
            return;
        };
        self.url = self.current_url_of(&engine);
        self.suspended.history = engine.history();
        self.suspended.keyboard_host = false;
        engine.set_keyboard_host(None);
        engine.set_visible(false);
        drop(engine);
        // What lived in the page goes with it, as when a page leaves.
        self.loading = false;
        self.progress = Default::default();
        self.mesurer_load_started(cx);
        self.design_load_started();
        cx.notify();
    }

    /// Builds the web view off screen if the tab gave it up (or never had one), at `bounds` (the
    /// tab on show's page area, so the page lays out as it would there): an agent's op on a tab
    /// not on show runs on it. Hidden, never key, no keyboard host until it shows.
    pub fn wake(
        &mut self,
        bounds: Option<Bounds<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.engine.is_some() || self.error.is_some() {
            return;
        }
        let bounds = self
            .bounds
            .or(bounds)
            .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.))));
        self.start(bounds, window, cx);
    }

    /// The page area the view was last placed in, if it was.
    pub fn page_bounds(&self) -> Option<Bounds<Pixels>> {
        self.bounds
    }

    fn current_url_of(&self, engine: &super::engine::Engine) -> String {
        engine
            .url()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| self.url.clone())
    }
}
