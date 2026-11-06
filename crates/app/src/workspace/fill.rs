//! A panel filling the window: the terminal dock, or the browser's or the canvas's tab (no sidebar,
//! no other area, no tab bar, no status bar), and the layout it replaced put back as it was. ⇧⌘↩
//! fills the window with the panel the user is in and puts it back; ⌃⌘1, ⌃⌘2 and ⌃⌘3 go straight to
//! the terminal, the browser and the canvas; Escape puts it back where the panel has no use for
//! Escape itself. While a panel fills the window, the title bar shows a Restore Layout button, and
//! for a moment the key that does the same.

use std::time::Duration;

use super::*;

/// How long the title bar says which key restores the layout, after a panel fills the window.
const HINT_FOR: Duration = Duration::from_millis(3_000);

impl Workspace {
    /// The panel that fills the window now: the terminal while it is all the window shows, else
    /// the tab that fills it ([`Self::filled_tab`]), as the browser or the canvas.
    pub(crate) fn filled(&self) -> Option<FillPanel> {
        if self.welcome.is_some() {
            return None;
        }
        if self.layout == TERMINAL_ONLY {
            return Some(FillPanel::Terminal);
        }
        let id = self.filled_tab?;
        let view = match self.tabs.get(self.active).map(|t| &t.item) {
            Some(PaneItem::Other { view, .. }) if view.entity_id() == id => view.clone(),
            _ => return None,
        };
        if view.clone().downcast::<BrowserView>().is_ok() {
            Some(FillPanel::Browser)
        } else if view.downcast::<BoardView>().is_ok() {
            Some(FillPanel::Canvas)
        } else {
            None
        }
    }

    /// ⇧⌘↩: puts the layout back when a panel fills the window; else fills it with the panel the
    /// user is in: the terminal while it has the keyboard, else the browser or the canvas while it
    /// is the tab on screen, else the terminal.
    pub(super) fn toggle_maximize_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filled().is_some() {
            self.restore_layout(cx);
            return;
        }
        let terminal_focused = self.terminal.read(cx).has_focus(window, cx);
        let center_shown = self.welcome.is_none() && self.layout.center;
        let panel = if terminal_focused || !center_shown {
            FillPanel::Terminal
        } else if self.canvas_active() {
            FillPanel::Canvas
        } else if self.active_browser_tabs().is_some() {
            FillPanel::Browser
        } else {
            FillPanel::Terminal
        };
        self.toggle_fill(panel, Origin::User, window, cx);
    }

    /// Fills the window with `panel`, opening the browser or the canvas when it isn't open, or
    /// puts the layout back when `panel` fills it already. Another panel filling the window gives
    /// way first, and the layout to come back to stays the one before either. For the user the
    /// panel takes the keyboard; an agent leaves it where it is.
    pub(crate) fn toggle_fill(
        &mut self,
        panel: FillPanel,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let filled = self.filled();
        if filled == Some(panel) {
            self.restore_layout(cx);
            return;
        }
        // From the welcome screen, the layout to come back to is the default one.
        self.leave_welcome(Layout::default(), cx);
        let before = match filled {
            Some(_) => self.unmaximized.take(),
            None => Some(self.layout),
        };
        match panel {
            FillPanel::Terminal => {
                self.filled_tab = None;
                self.layout = TERMINAL_ONLY;
                if origin == Origin::User {
                    self.terminal.update(cx, |t, cx| t.focus_active(window, cx));
                }
            }
            FillPanel::Browser => {
                let browser = self.open_browser(None, window, cx);
                if origin == Origin::User
                    && !browser
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                {
                    window.focus(&browser.read(cx).focus_handle(cx), cx);
                }
                self.fill_tab(browser.entity_id());
            }
            FillPanel::Canvas => {
                let Some(board) = self.open_canvas(cx) else {
                    self.unmaximized = before.filter(|_| filled.is_some());
                    return;
                };
                if origin == Origin::User {
                    board.update(cx, |board, cx| board.take_keyboard(cx));
                }
                self.fill_tab(board.entity_id());
            }
        }
        self.unmaximized = before;
        self.show_fill_hint(cx);
        cx.notify();
    }

    /// The canvas's own Maximize button, on canvas `board`, which is on screen: as ⇧⌘↩ on it.
    pub(super) fn toggle_canvas_maximized(&mut self, board: EntityId, cx: &mut Context<Self>) {
        match self.filled() {
            Some(FillPanel::Canvas) => self.restore_layout(cx),
            filled => {
                if self.tab_view(self.active) != Some(board) {
                    return;
                }
                let before = match filled {
                    Some(_) => self.unmaximized.take(),
                    None => Some(self.layout),
                };
                self.fill_tab(board);
                self.unmaximized = before;
                self.show_fill_hint(cx);
                cx.notify();
            }
        }
    }

    /// What `app_maximize` answers: the panel that fills the window now, if any.
    pub(super) fn describe_fill(&self) -> serde_json::Value {
        let panel = self.filled().map(|p| match p {
            FillPanel::Terminal => "terminal",
            FillPanel::Browser => "browser",
            FillPanel::Canvas => "canvas",
        });
        serde_json::json!({
            "maximized": panel.is_some(),
            "panel": panel,
            "layout": {
                "sidebar": self.layout.sidebar,
                "center": self.layout.center,
                "terminal": self.layout.terminal,
            },
        })
    }

    /// The terminal's Maximize button and `app_maximize_terminal`.
    pub(super) fn toggle_terminal_maximized(
        &mut self,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_fill(FillPanel::Terminal, origin, window, cx);
    }

    /// The tab on screen, `id`, fills the window: the tabs alone, without their tab bar.
    fn fill_tab(&mut self, id: EntityId) {
        self.filled_tab = Some(id);
        self.layout = CENTER_ONLY;
    }

    /// Puts back the layout a panel filling the window replaced: the same areas, at the same sizes
    /// (the splits keep theirs while hidden, [`Self::splits`]).
    pub(crate) fn restore_layout(&mut self, cx: &mut Context<Self>) {
        self.filled_tab = None;
        self.layout = self.unmaximized.take().unwrap_or_default();
        self.fill_hint = false;
        cx.notify();
    }

    /// Escape while a panel fills the window: puts the layout back, unless the panel has a use
    /// for Escape itself, which then gets it: the terminal (its programs) and the canvas (it clears
    /// the selection, ends a text edit, drops a tool). A text field, a menu or a dialog takes its
    /// Escape first, by its own binding; a page with the keyboard gets its keys from AppKit, Escape
    /// among them. So in the browser Escape restores the layout from its toolbar and its panels.
    pub(super) fn escape_fill(&mut self, cx: &mut Context<Self>) {
        match self.filled() {
            Some(FillPanel::Browser) => self.restore_layout(cx),
            _ => cx.propagate(),
        }
    }

    /// Shows the hint that says which key puts the layout back, for [`HINT_FOR`].
    fn show_fill_hint(&mut self, cx: &mut Context<Self>) {
        self.fill_hint = true;
        self._fill_hint_reset = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HINT_FOR).await;
            let _ = this.update(cx, |this, cx| {
                this.fill_hint = false;
                cx.notify();
            });
        });
    }

    /// Ends a tab's fill once something else changed what is on screen (⌘B, ⌘J, another tab, the
    /// welcome screen): a tab fills the window only while it is the one showing, on its own. Each
    /// render checks, because the layout and the active tab change in many places.
    pub(super) fn sync_filled(&mut self) {
        let shown = (self.welcome.is_none() && self.layout.center)
            .then(|| self.tab_view(self.active))
            .flatten();
        settle_tab_fill(
            &mut self.filled_tab,
            &mut self.unmaximized,
            self.layout,
            shown,
        );
    }

    /// The Restore Layout button the title bar shows while a panel fills the window, after the
    /// key that does the same while the hint shows (`⇧⌘↩ to exit`).
    pub(super) fn render_fill_exit(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.filled()?;
        let keys = crate::keymap::keys_for(&ToggleMaximizePane, "Workspace", window);
        let hint = (self.fill_hint && !keys.is_empty()).then(|| format!("{keys} to exit"));
        Some(
            h_flex()
                .flex_none()
                .gap_2()
                .children(hint.map(|hint| {
                    div()
                        .id("fill-hint")
                        .debug_selector(|| "fill-hint".into())
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(hint)
                }))
                .child(
                    Button::new("restore-layout")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Minimize)
                        .label("Restore Layout")
                        .accessibility_label("Restore Layout")
                        .tooltip_with_action(
                            "Restore Layout",
                            &ToggleMaximizePane,
                            Some("Workspace"),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.restore_layout(cx))),
                )
                .into_any_element(),
        )
    }
}

/// Ends a tab's fill of the window (`filled`, the tab's view) once the window shows something else:
/// `layout` is no longer the tabs alone (⌘B, ⌘J), or another tab is on screen (`shown`; the
/// welcome screen, no tab, or the tabs hidden: `None`). The tab bar comes back with it, and the
/// layout stays as it was changed to.
pub(super) fn settle_tab_fill<T: PartialEq + Copy>(
    filled: &mut Option<T>,
    unmaximized: &mut Option<Layout>,
    layout: Layout,
    shown: Option<T>,
) {
    if filled.is_some() && !(shown == *filled && layout == CENTER_ONLY) {
        *filled = None;
        *unmaximized = None;
    }
}
