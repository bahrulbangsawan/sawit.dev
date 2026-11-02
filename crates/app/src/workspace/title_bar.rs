//! The title bar's controls: the project switcher, and on the right the pages and layout toggles.

use super::*;

impl Workspace {
    /// One chip per open project (click to switch, × to close) and a +
    /// that adds another; just the tagline while no project is open.
    pub(super) fn render_project_switcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let roots: Vec<Option<PathBuf>> = self.projects.iter().map(|p| p.root(cx)).collect();
        if roots.iter().all(Option::is_none) {
            return div()
                .text_color(cx.theme().muted_foreground)
                .child("Your local dev workspace")
                .into_any_element();
        }
        let only = roots.len() == 1;
        let mut colors = 0;
        let chips = roots.into_iter().enumerate().filter_map(|(ix, root)| {
            let root = root?;
            let color = crate::tab_groups::group_color(colors, cx);
            colors += 1;
            let active = ix == self.project;
            Some(
                h_flex()
                    .id(("project", ix))
                    .role(Role::Tab)
                    .aria_label(project_name(&root))
                    .aria_selected(active)
                    .flex_none()
                    .h(px(24.))
                    .pl_2()
                    .pr_0p5()
                    .gap_1p5()
                    .rounded(cx.theme().radius)
                    .cursor_pointer()
                    .when(active, |el| el.bg(cx.theme().list_active))
                    .when(!active, |el| {
                        el.text_color(cx.theme().muted_foreground)
                            .hover(|el| el.bg(cx.theme().list_hover))
                    })
                    .child(div().size(px(8.)).rounded_full().bg(color))
                    .child(div().max_w(px(160.)).truncate().child(project_name(&root)))
                    .child(
                        Button::new(("close-project", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .accessibility_label(format!("Close {}", project_name(&root)))
                            .tooltip(if only {
                                "Close Project"
                            } else {
                                "Close Project and Its Tabs"
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_project(ix, window, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.leave_welcome(PANEL_AND_TABS, cx);
                        this.activate_project(ix, cx);
                    })),
            )
        });
        h_flex()
            .id("project-switcher")
            .role(Role::TabList)
            .aria_label("Projects")
            .min_w_0()
            .gap_1()
            .overflow_x_scroll()
            .children(chips)
            .child(
                Button::new("add-project")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Plus)
                    .accessibility_label("Open Another Project")
                    .tooltip_with_action(
                        "Open Another Project in This Window",
                        &AddProject,
                        Some("Workspace"),
                    )
                    .on_click(|_, window, cx| {
                        crate::cli::prompt_open_folder(Some(window.window_handle()), cx)
                    }),
            )
            .into_any_element()
    }

    /// The title bar's right side: an update waiting, the pages a click
    /// opens, then the layout toggles, each set in its own group.
    pub(super) fn render_layout_toggles(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let l = self.layout;
        let group = || {
            h_flex()
                .flex_none()
                .p(px(2.))
                .gap(px(2.))
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
        };
        let toggle = |id: &'static str,
                      icon: Lucide,
                      tooltip: &'static str,
                      action: &dyn Action,
                      on: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .accessibility_label(tooltip)
                .tooltip_with_action(tooltip, action, Some("Workspace"))
                .selected(on)
        };
        let pending = self.updater.read(cx).pending().map(|r| r.version);
        h_flex()
            .flex_none()
            .gap_2()
            .children(pending.map(|v| {
                Button::new("restart-update")
                    .primary()
                    .xsmall()
                    .icon(Lucide::Download)
                    .label("Restart to Update")
                    .tooltip_with_action(
                        format!("Install Sawit {v} and Relaunch"),
                        &InstallUpdate,
                        Some("Workspace"),
                    )
                    .on_click(|_, _, cx| crate::update::restart(cx))
            }))
            .child(
                group()
                    .child(
                        Button::new("open-canvas")
                            .ghost()
                            .xsmall()
                            .icon(crate::board::ICON)
                            .label("Canvas")
                            .tooltip_with_action(
                                "Brainstorm on a Whiteboard",
                                &OpenCanvas,
                                Some("Workspace"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.show_canvas(cx))),
                    )
                    .child(
                        Button::new("open-browser")
                            .ghost()
                            .xsmall()
                            .icon(crate::browser::ICON)
                            .label("Browser")
                            .tooltip("Preview Your App in a Browser an Agent Can Drive")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_browser(None, window, cx);
                            })),
                    )
                    .child(
                        Button::new("open-tools")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Package)
                            .label("Tools")
                            .tooltip_with_action(
                                "Install Dev Tools: Claude Code, Node.js, Bun, uv and More",
                                &OpenDevTools,
                                Some("Workspace"),
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_tools_page(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("open-mcp")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Plug)
                            .label("MCP")
                            .tooltip_with_action(
                                "Install MCP Servers into Claude, Codex and More",
                                &OpenMcp,
                                Some("Workspace"),
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_mcp_page(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("open-skills")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Sparkles)
                            .label("Skills")
                            .tooltip(
                                "Install Agent Skills: Superpowers, Cloudflare, MCP Apps and More",
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_skills_page(window, cx)
                                }),
                            ),
                    )
                    .child(
                        Button::new("open-new-project")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::FolderPlus)
                            .label("New")
                            .accessibility_label("New Project")
                            .tooltip_with_action(
                                "New Project from a TypeScript Stack",
                                &OpenNewProject,
                                Some("Workspace"),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_new_project_page(window, cx)
                            })),
                    )
                    .child(
                        Button::new("open-keymap")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Keyboard)
                            .accessibility_label("Keymap Editor")
                            .tooltip_with_action(
                                "Keymap Editor",
                                &OpenKeymapEditor,
                                Some("Workspace"),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_keymap_editor(window, cx)
                            })),
                    ),
            )
            .child(
                group()
                    .child(
                        toggle(
                            "toggle-sidebar",
                            Lucide::PanelLeft,
                            "Sidebar",
                            &ToggleSidebar,
                            l.sidebar,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.toggle_area(Area::Sidebar, cx)),
                        ),
                    )
                    .child(
                        toggle(
                            "toggle-center",
                            Lucide::PanelTop,
                            "Tabs & Tables",
                            &ToggleCenter,
                            l.center,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_area(Area::Center, cx))),
                    )
                    .child(
                        toggle(
                            "toggle-terminal",
                            Lucide::PanelBottom,
                            "Terminal",
                            &ToggleTerminal,
                            l.terminal,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.toggle_area(Area::Terminal, cx)),
                        ),
                    ),
            )
    }
}
