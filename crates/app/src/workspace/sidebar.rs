//! The sidebar: its row of panel buttons and the panel it shows.

use super::render::cached;
use super::*;

impl Workspace {
    /// Shows `tab`'s panel in the sidebar, opening the sidebar if it's hidden.
    pub(super) fn show_panel(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        self.leave_welcome(PANEL_AND_TABS, cx);
        self.sidebar = tab;
        if !self.layout.sidebar {
            self.layout.toggle(Area::Sidebar);
        }
        if tab == SidebarTab::ShellConfig {
            let roots = self.roots(cx);
            self.shell_config_panel
                .update(cx, |p, cx| p.set_projects(roots, cx));
        }
        cx.notify();
    }

    /// While the Database panel shows, its store looks for the Cloudflare D1 databases of this
    /// window's projects (nothing to do while they stay the same).
    pub(super) fn sync_db_projects(&mut self, cx: &mut Context<Self>) {
        if self.sidebar == SidebarTab::Database && self.layout.sidebar {
            let roots = self.roots(cx);
            self.db.update(cx, |s, cx| s.set_projects(roots, cx));
        }
    }

    pub(super) fn render_sidebar_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tabs: [(&'static str, Icon, &'static str, SidebarTab, &dyn Action); 10] = [
            (
                "side-servers",
                Lucide::Server.into(),
                "PostgreSQL Instances",
                SidebarTab::Servers,
                &ShowInstances,
            ),
            (
                "side-files",
                IconName::Folder.into(),
                "Files",
                SidebarTab::Files,
                &ShowFiles,
            ),
            (
                "side-git",
                Lucide::GitBranch.into(),
                "Git",
                SidebarTab::Git,
                &ShowGit,
            ),
            (
                "side-db",
                Lucide::Database.into(),
                "Database",
                SidebarTab::Database,
                &ShowDatabase,
            ),
            (
                "side-run",
                Lucide::Play.into(),
                "Run Scripts",
                SidebarTab::Run,
                &ShowRun,
            ),
            (
                "side-ports",
                Lucide::Radio.into(),
                "Listening Ports",
                SidebarTab::Ports,
                &ShowPorts,
            ),
            (
                "side-network",
                IconName::Globe.into(),
                "Network Diagnosis",
                SidebarTab::Network,
                &ShowNetwork,
            ),
            (
                "side-ssh",
                Lucide::SquareTerminal.into(),
                "SSH Servers",
                SidebarTab::Ssh,
                &ShowSsh,
            ),
            (
                "side-shell-config",
                Lucide::FileTerminal.into(),
                "Config Files",
                SidebarTab::ShellConfig,
                &ShowConfigFiles,
            ),
            (
                "side-prompts",
                Lucide::MessageSquareText.into(),
                "Prompts",
                SidebarTab::Prompts,
                &ShowPrompts,
            ),
        ];
        let current = self.sidebar;
        h_flex()
            .h(px(30.))
            .px_1()
            .gap_0p5()
            .bg(cx.theme().sidebar)
            .border_b_1()
            .border_color(cx.theme().border)
            .children(tabs.into_iter().map(|(id, icon, tooltip, which, action)| {
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .role(Role::Tab)
                    .accessibility_label(tooltip)
                    .tooltip_with_action(tooltip, action, Some("Workspace"))
                    .selected(current == which)
                    .on_click(cx.listener(move |this, _, _, cx| this.show_panel(which, cx)))
            }))
    }

    pub(super) fn render_sidebar(&self) -> AnyElement {
        let view: AnyView = match self.sidebar {
            SidebarTab::Servers => self.server_list.clone().into(),
            SidebarTab::Files => self.current().files.clone().into(),
            SidebarTab::Git => self.current().git_panel.clone().into(),
            SidebarTab::Database => self.db_panel.clone().into(),
            SidebarTab::Run => self.current().run_panel.clone().into(),
            SidebarTab::Ports => self.ports_panel.clone().into(),
            SidebarTab::Network => self.network_panel.clone().into(),
            SidebarTab::Prompts => self.prompts_panel.clone().into(),
            SidebarTab::Ssh => self.ssh_panel.clone().into(),
            SidebarTab::ShellConfig => self.shell_config_panel.clone().into(),
        };
        cached(view)
    }
}
