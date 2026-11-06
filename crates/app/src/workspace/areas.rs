//! What the window shows: the welcome screen until something is opened, then the sidebar, the tabs
//! and the terminal, each shown or hidden. A panel filling the window is `fill`'s.

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
            WelcomeEvent::InstallGitHubCli => self.set_up_github_cli(window, cx),
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
            self.filled_tab,
            self.project,
            self.sidebar,
            self.tab_view(self.active),
        )
    }
}
