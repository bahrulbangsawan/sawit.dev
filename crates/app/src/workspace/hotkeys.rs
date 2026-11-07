//! B5's area keys: ⌘T, ⌘U, ⌘B and ⌘E open and close the terminal, the canvas, the browser and
//! the folder panel (with ⇧, `fill`'s Maximize actions fill the window with them); ⌘N makes a
//! file; ⌥Tab moves the keyboard between the terminal and the tabs.

use super::*;

/// What [`Workspace::toggle_tab_area`] shows or hides: the browser's tab or the canvas's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TabArea {
    Browser,
    Canvas,
}

impl Workspace {
    /// Whether `area`'s tab is the one on screen: the tabs show and it is the active tab.
    pub(super) fn tab_area_shown(&self, area: TabArea) -> bool {
        if self.welcome.is_some() || !self.layout.center {
            return false;
        }
        match area {
            TabArea::Browser => self.active_browser_tabs().is_some(),
            TabArea::Canvas => self.canvas_active(),
        }
    }

    /// ⌘B and ⌘U: shows `area`'s tab, opening it if the project has none, with the keyboard; or,
    /// when it is on screen, hides it: the tab before it shows instead (the one after, for the
    /// first), and with no other tab the tabs hide. The keyboard goes to what is left, never to
    /// nothing ([`Self::focus_panel`]).
    pub(super) fn toggle_tab_area(
        &mut self,
        area: TabArea,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_area_shown(area) {
            if self.filled().is_some() {
                self.restore_layout(cx);
            }
            if self.tabs.len() > 1 {
                let ix = match self.active {
                    0 => 1,
                    n => n - 1,
                };
                self.show_tab(ix, cx);
                self.focus_tabs(window, cx);
            } else {
                self.toggle_area(Area::Center, cx);
                self.focus_panel(Panel::Terminal, window, cx);
            }
            cx.notify();
            return;
        }
        self.show_center(cx);
        match area {
            TabArea::Browser => {
                let browser = self.open_browser(None, window, cx);
                window.focus(&browser.read(cx).focus_handle(cx), cx);
            }
            TabArea::Canvas => self.show_canvas(cx),
        }
        cx.notify();
    }

    /// ⌘T: the terminal's dock shown or hidden. Hidden, the keyboard goes to the tabs.
    pub(super) fn toggle_terminal_area(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was_shown = self.welcome.is_none() && self.layout.terminal;
        if self.filled() == Some(FillPanel::Terminal) {
            self.restore_layout(cx);
        }
        self.toggle_area(Area::Terminal, cx);
        if was_shown {
            if self.layout.center {
                self.focus_tabs(window, cx);
            }
        } else {
            self.terminal.update(cx, |t, cx| t.focus_active(window, cx));
        }
    }

    /// ⌘N: a new file in the folder selected in the file tree, else in the project's root, its
    /// name ready to type, as the tree's own New File makes it. The folder panel shows the tree.
    pub(super) fn new_file_in_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_panel(SidebarTab::Files, cx);
        let files = self.current().files.clone();
        files.update(cx, |files, cx| files.new_file(window, cx));
    }

    /// ⌥Tab and ⌥⇧Tab: the keyboard to the other panel, the terminal or the tabs, showing it if
    /// it is hidden. With the two of them, next and previous are the same.
    pub(super) fn focus_other_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let in_terminal = self.terminal.read(cx).has_focus(window, cx);
        let to = match in_terminal {
            true => Panel::Tabs,
            false => Panel::Terminal,
        };
        self.focus_panel(to, window, cx);
    }

    /// Gives the keyboard to `panel`, showing it first.
    fn focus_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        match panel {
            Panel::Terminal => {
                if self.welcome.is_some() || !self.layout.terminal {
                    self.toggle_area(Area::Terminal, cx);
                }
                self.terminal.update(cx, |t, cx| t.focus_active(window, cx));
            }
            Panel::Tabs => {
                self.show_center(cx);
                self.focus_tabs(window, cx);
            }
        }
        cx.notify();
    }

    /// The keyboard to the tab on screen: a file's text, the canvas, the browser, else the
    /// workspace itself, which holds every shortcut.
    fn focus_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.tabs.get(self.active).map(|t| &t.item) {
            Some(PaneItem::File(editor)) => {
                let handle = editor.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            Some(PaneItem::Other { view, .. }) => {
                let view = view.clone();
                if let Ok(board) = view.clone().downcast::<BoardView>() {
                    board.update(cx, |board, cx| board.take_keyboard(cx));
                } else if let Ok(tabs) = view.downcast::<BrowserTabs>() {
                    let handle = tabs.read(cx).focus_handle(cx);
                    window.focus(&handle, cx);
                } else {
                    window.focus(&self.focus, cx);
                }
            }
            _ => window.focus(&self.focus, cx),
        }
    }
}

/// The panels ⌥Tab moves between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Terminal,
    Tabs,
}
