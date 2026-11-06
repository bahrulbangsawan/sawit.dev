//! ⌘F in a tab with no text to search (an image, a video, a page with no find of its own): the
//! find bar says "Search isn't available here", rather than nothing happening. Each view that
//! finds takes ⌘F before it reaches the workspace.

use super::*;

impl Workspace {
    /// ⌘F that no view under the keyboard took: the bar above the tab on screen says there is
    /// nothing to search, when the keyboard is in the tabs (or nowhere); elsewhere (the sidebar,
    /// the terminal dock's buttons) ⌘F does nothing.
    pub(super) fn find_unavailable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let in_tabs = self.center_area.contains_focused(window, cx)
            || self.focus.is_focused(window)
            || window.focused(cx).is_none();
        if !in_tabs || self.welcome.is_some() || !self.layout.center {
            return;
        }
        // A tab that finds, with the keyboard not yet in it (a PDF shown from its tab or by an
        // agent, a canvas): ⌘F is its, as the user's press, and the keyboard goes into it.
        if let Some(handle) = self.find_target(cx)
            && !handle.contains_focused(window, cx)
        {
            window.focus(&handle, cx);
            window.dispatch_action(Box::new(crate::find::Find), cx);
            return;
        }
        self.finder_tab = self.tab_view(self.active);
        self.finder.open(
            None,
            None,
            false,
            |this: &mut Self, event, window, cx| {
                if *event == crate::find::FindEvent::Close {
                    this.close_unavailable(window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// The keyboard's place in the tab on screen, when the tab can find.
    fn find_target(&self, cx: &App) -> Option<FocusHandle> {
        match &self.tabs.get(self.active)?.item {
            PaneItem::File(editor) => Some(editor.read(cx).focus_handle(cx)),
            PaneItem::Other { view, .. } => {
                let view = view.clone();
                if let Ok(pdf) = view.clone().downcast::<PdfView>() {
                    Some(pdf.read(cx).focus.clone())
                } else if let Ok(board) = view.clone().downcast::<BoardView>() {
                    Some(board.read(cx).focus_handle(cx))
                } else if let Ok(browser) = view.downcast::<BrowserView>() {
                    Some(browser.read(cx).focus_handle(cx))
                } else {
                    None
                }
            }
            PaneItem::Server => None,
        }
    }

    /// Escape in that bar, or its ✕.
    pub(super) fn close_unavailable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = self.center_area.clone();
        if self.finder.close(&back, window, cx) {
            self.finder_tab = None;
            cx.notify();
        }
    }

    /// The bar, while the tab it opened over is still the one on screen.
    pub(super) fn render_unavailable(&mut self) -> Option<AnyElement> {
        if self.finder.is_open() && self.finder_tab != self.tab_view(self.active) {
            self.finder = Default::default();
            self.finder_tab = None;
        }
        self.finder.render()
    }
}
