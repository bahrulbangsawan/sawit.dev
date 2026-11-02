//! What the window shows: the welcome screen until something is opened, then the sidebar, the tabs
//! and the terminal, each shown or hidden, or the terminal or a canvas filling the window.

use super::*;

impl Workspace {
    /// Swaps the welcome screen for the panels in `layout`; once it is gone
    /// the layout is left alone.
    pub(super) fn leave_welcome(&mut self, layout: Layout, cx: &mut Context<Self>) {
        if self.welcome.take().is_some() {
            self.layout = layout;
            cx.notify();
        }
    }

    pub(super) fn on_welcome_event(
        &mut self,
        event: &WelcomeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = window.window_handle();
        match event {
            WelcomeEvent::OpenFolder => crate::cli::prompt_open_folder(Some(handle), cx),
            WelcomeEvent::OpenRecent(dir) => {
                // Reads every workspace, this one included: after this update.
                let dir = dir.clone();
                cx.defer(move |cx| crate::cli::open_folder_in(dir, Some(handle), cx));
            }
            WelcomeEvent::NewInstance => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.sidebar = SidebarTab::Servers;
                self.servers.update(cx, |s, cx| s.show_create(cx));
            }
            WelcomeEvent::OpenServer(name) => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.sidebar = SidebarTab::Servers;
                self.servers.update(cx, |s, cx| s.select(name, cx));
            }
            WelcomeEvent::OpenDatabase(name) => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.sidebar = SidebarTab::Database;
                self.db.update(cx, |s, cx| s.ensure_connecting(name, cx));
            }
            WelcomeEvent::OpenTerminal => {
                self.leave_welcome(TERMINAL_ONLY, cx);
                self.terminal.update(cx, |t, cx| t.focus_active(window, cx));
            }
            WelcomeEvent::NewProject => self.open_new_project_page(window, cx),
            WelcomeEvent::Dismiss => self.leave_welcome(PANEL_AND_TABS, cx),
            WelcomeEvent::SignInToGitHub => self.sign_in_to_github(window, cx),
            WelcomeEvent::OpenGitHub => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.open_github(window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn toggle_area(&mut self, area: Area, cx: &mut Context<Self>) {
        // From the welcome screen, toggles build up from just the tabs.
        self.leave_welcome(CENTER_ONLY, cx);
        self.layout.toggle(area);
        cx.notify();
    }

    pub(super) fn show_center(&mut self, cx: &mut Context<Self>) {
        self.leave_welcome(PANEL_AND_TABS, cx);
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
    }

    /// What the workspace shows now ([`Shown`]).
    pub(super) fn shown(&self) -> Shown {
        (
            self.welcome.is_some(),
            self.layout,
            self.canvas_maximized,
            self.project,
            self.sidebar,
            self.tab_view(self.active),
        )
    }

    /// Fills the window with the terminal, or puts back the layout it
    /// replaced (everything, if that was terminal-only too). For the user
    /// the terminal takes the keyboard; an agent leaves it where it is.
    pub(super) fn toggle_terminal_maximized(
        &mut self,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.canvas_maximized = false;
        if self.layout == TERMINAL_ONLY {
            self.layout = self.unmaximized.take().unwrap_or_default();
        } else {
            self.unmaximized = Some(self.layout);
            self.layout = TERMINAL_ONLY;
        }
        if origin == Origin::User {
            self.terminal.update(cx, |t, cx| t.focus_active(window, cx));
        }
        cx.notify();
    }

    /// ⇧⌘↩: the canvas fills the window while it is the tab on screen and
    /// the terminal doesn't have the keyboard; otherwise the terminal does.
    pub(super) fn toggle_maximize_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let terminal_focused = self.terminal.read(cx).has_focus(window, cx);
        let canvas_shown = self.welcome.is_none() && self.layout.center && self.canvas_active();
        if canvas_shown && !terminal_focused || self.canvas_maximized {
            self.toggle_canvas_maximized(cx);
        } else {
            self.toggle_terminal_maximized(Origin::User, window, cx);
        }
    }

    /// Fills the window with the canvas (hiding the tab bar too), or puts
    /// back the layout it replaced, like the terminal's maximize.
    pub(super) fn toggle_canvas_maximized(&mut self, cx: &mut Context<Self>) {
        toggle_canvas_fill(
            &mut self.canvas_maximized,
            &mut self.layout,
            &mut self.unmaximized,
        );
        cx.notify();
    }

    /// Ends "fill window" once something else changed what is on screen (⌘B, ⌘J, another tab,
    /// the welcome screen): the canvas fills the window only while its tab is the one showing, on
    /// its own. Each render checks, because the layout and the active tab change in many places.
    pub(super) fn sync_canvas_maximized(&mut self) {
        let shown = self.welcome.is_none() && self.layout.center && self.canvas_active();
        settle_canvas_fill(
            &mut self.canvas_maximized,
            &mut self.unmaximized,
            self.layout,
            shown,
        );
    }
}

/// ⇧⌘↩ on the canvas: fills the window with it (`maximized`: the tabs alone, without their tab
/// bar), or puts back the layout it replaced. `unmaximized` is the layout to go back to, which
/// the terminal's maximize shares.
pub(super) fn toggle_canvas_fill(
    maximized: &mut bool,
    layout: &mut Layout,
    unmaximized: &mut Option<Layout>,
) {
    if *maximized && *layout == CENTER_ONLY {
        *maximized = false;
        *layout = unmaximized.take().unwrap_or_default();
    } else {
        if !*maximized && *layout != TERMINAL_ONLY {
            *unmaximized = Some(*layout);
        }
        *maximized = true;
        *layout = CENTER_ONLY;
    }
}

/// Ends the canvas's fill-window (`maximized`) once the window shows something else: `layout` is
/// no longer the tabs alone (⌘B, ⌘J), or the canvas isn't the tab on screen (`canvas_shown`;
/// another tab, the welcome screen). The tab bar comes back with it, and the layout stays as it
/// was changed to.
pub(super) fn settle_canvas_fill(
    maximized: &mut bool,
    unmaximized: &mut Option<Layout>,
    layout: Layout,
    canvas_shown: bool,
) {
    if *maximized && !(canvas_shown && layout == CENTER_ONLY) {
        *maximized = false;
        *unmaximized = None;
    }
}
