//! Drawing the window: the workspace's actions and title bar, its body and status bar, and each pane
//! mounted so that it re-renders only when it changes.

use super::*;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        A11Y_ACTIVE.set(window.is_a11y_active() && needs_whole_tree());
        if window.focused(cx).is_none() {
            // Keep workspace shortcuts working when nothing inside has focus.
            self.focus.focus(window, cx);
        }
        self.sync_canvas_maximized();
        self.sync_boards(window, cx);
        self.sync_browser(cx);
        self.sync_db_projects(cx);
        // After the canvases: one shown again takes its keyboard back itself, later in the frame.
        let pages: Vec<FocusHandle> = self
            .browsers
            .iter()
            .filter_map(|b| b.upgrade())
            .map(|b| b.read(cx).focus_handle(cx))
            .collect();
        let shown = self.shown();
        self.keyboard.give_back(
            shown,
            |back, cx| !pages.contains(back) && !crate::keymap::is_recorder(back, cx),
            window,
            cx,
        );
        v_flex()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &CloseActiveTab, w, cx| {
                this.on_close_action(CloseOp::This, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseOtherTabs, w, cx| {
                this.on_close_action(CloseOp::Others, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseTabsLeft, w, cx| {
                this.on_close_action(CloseOp::Left, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseTabsRight, w, cx| {
                this.on_close_action(CloseOp::Right, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseCleanTabs, w, cx| {
                this.on_close_action(CloseOp::Clean, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseAllTabs, w, cx| {
                this.on_close_action(CloseOp::All, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_area(Area::Sidebar, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, _, cx| this.toggle_area(Area::Terminal, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleCenter, _, cx| this.toggle_area(Area::Center, cx)),
            )
            .on_action(cx.listener(|this, _: &TogglePinTab, _, cx| {
                let ix = this.active;
                this.toggle_pin(ix, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleMaximizePane, w, cx| this.toggle_maximize_pane(w, cx)),
            )
            .on_action(cx.listener(|this, _: &NextTab, _, cx| this.cycle_tab(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousTab, _, cx| this.cycle_tab(-1, cx)))
            .on_action(cx.listener(|this, _: &NextProject, _, cx| this.cycle_project(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousProject, _, cx| this.cycle_project(-1, cx)))
            .on_action(cx.listener(|_, _: &AddProject, w, cx| {
                crate::cli::prompt_open_folder(Some(w.window_handle()), cx)
            }))
            .on_action(cx.listener(|this, _: &CloseProject, w, cx| {
                let ix = this.project;
                this.close_project(ix, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyProjectPath, _, cx| {
                if let Some(root) = this.root(cx) {
                    this.copy_project_path(root.display().to_string(), cx)
                }
            }))
            .on_action(cx.listener(|this, _: &NewTerminal, w, cx| this.new_terminal(w, cx)))
            .on_action(cx.listener(|this, _: &ShowInstances, _, cx| {
                this.show_panel(SidebarTab::Servers, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowFiles, _, cx| this.show_panel(SidebarTab::Files, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowGit, _, cx| this.show_panel(SidebarTab::Git, cx)))
            .on_action(cx.listener(|this, _: &ShowDatabase, _, cx| {
                this.show_panel(SidebarTab::Database, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowRun, _, cx| this.show_panel(SidebarTab::Run, cx)))
            .on_action(
                cx.listener(|this, _: &ShowPorts, _, cx| this.show_panel(SidebarTab::Ports, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowNetwork, _, cx| {
                    this.show_panel(SidebarTab::Network, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ShowSsh, _, cx| this.show_panel(SidebarTab::Ssh, cx)))
            .on_action(cx.listener(|this, _: &ShowConfigFiles, _, cx| {
                this.show_panel(SidebarTab::ShellConfig, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowPrompts, _, cx| {
                    this.show_panel(SidebarTab::Prompts, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenCanvas, _, cx| this.show_canvas(cx)))
            .on_action(cx.listener(|this, _: &OpenBrowser, w, cx| {
                this.open_browser(None, w, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSkills, w, cx| this.open_skills_page(w, cx)))
            .on_action(cx.listener(|this, _: &OpenDevTools, w, cx| this.open_tools_page(w, cx)))
            .on_action(
                cx.listener(|this, _: &OpenNewProject, w, cx| this.open_new_project_page(w, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenMcp, w, cx| this.open_mcp_page(w, cx)))
            .on_action(
                cx.listener(|this, _: &OpenLocalExplorer, w, cx| this.open_local_explorer(w, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenKeymapEditor, w, cx| {
                this.show_center(cx);
                this.open_keymap_editor(w, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenKeymapFile, w, cx| this.open_keymap_file(w, cx)))
            .on_action(cx.listener(|this, _: &OpenChangelog, _, cx| {
                this.show_center(cx);
                this.open_changelog_page(cx)
            }))
            // Every "Sign in" in the window runs the one sign-in, whose code the GitHub page
            // shows.
            .on_action(cx.listener(|this, _: &SignInToGitHub, w, cx| this.sign_in_to_github(w, cx)))
            .on_action(cx.listener(|this, _: &OpenGitHub, w, cx| {
                this.leave_welcome(PANEL_AND_TABS, cx);
                this.open_github(w, cx)
            }))
            .on_action(cx.listener(|this, _: &InstallUpdate, _, cx| {
                // Installs a pending update, else checks for one and shows
                // the changelog page, where the result appears.
                if this.updater.read(cx).pending().is_some() {
                    crate::update::restart(cx);
                } else {
                    this.updater.update(cx, |u, cx| u.check(cx));
                    this.show_center(cx);
                    this.open_changelog_page(cx);
                }
            }))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new()
                    // Fullscreen hides the traffic lights: drop the space kept for them.
                    .when(window.is_fullscreen(), |this| this.pl_0())
                    .child(
                        h_flex()
                            .size_full()
                            .pr_2()
                            .gap_2()
                            .text_sm()
                            .child(img(crate::assets::LOGO_MARK).size(px(16.)))
                            .child(div().font_semibold().child("Sawit"))
                            .child(self.render_project_switcher(cx))
                            .child(div().flex_1())
                            .child(self.render_layout_toggles(cx))
                            .child(crate::git::account::account_control(
                                &self.github,
                                super::pages_menu::pages,
                                cx,
                            )),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
            .child(self.render_status_bar(cx))
    }
}

impl Workspace {
    fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(welcome) = &self.welcome {
            return welcome.clone().into_any_element();
        }
        let l = self.layout;
        let maximized = l == TERMINAL_ONLY;
        self.terminal.update(cx, |t, cx| {
            if t.maximized != maximized {
                t.maximized = maximized;
                cx.notify();
            }
        });
        // The terminal stays alive while hidden; only its view is dropped.
        let main = match (l.center, l.terminal) {
            (true, true) => v_resizable("workspace-v")
                .child(resizable_panel().child(self.render_center(cx)))
                .child(
                    resizable_panel()
                        .size(px(240.))
                        .size_range(px(80.)..Pixels::MAX)
                        .child(cached(self.terminal.clone().into())),
                )
                .into_any_element(),
            (true, false) => self.render_center(cx).into_any_element(),
            _ => cached(self.terminal.clone().into()),
        };
        if !l.sidebar {
            return main;
        }
        h_resizable("workspace-h")
            .child(
                resizable_panel()
                    .size(px(240.))
                    .size_range(px(160.)..px(480.))
                    .child(
                        v_flex()
                            .size_full()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child(self.render_sidebar_tabs(cx))
                            .child(div().flex_1().min_h_0().child(self.render_sidebar())),
                    ),
            )
            .child(resizable_panel().child(main))
            .into_any_element()
    }

    fn render_center(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let show_tabs = !self.canvas_maximized;
        v_flex()
            .size_full()
            .when(show_tabs, |el| el.child(self.render_tab_bar(cx)))
            .child(div().flex_1().min_h_0().child(self.render_active_item(cx)))
    }

    fn render_active_item(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.tabs.get(self.active).map(|t| &t.item) {
            Some(PaneItem::File(editor)) => cached(editor.clone().into()),
            Some(PaneItem::Other { view, .. }) => cached(view.clone()),
            Some(PaneItem::Server) => cached(self.server_detail.clone().into()),
            None => Placeholder::new(Lucide::PanelTop, "No tabs open")
                .detail("Pick an instance, file or table in the sidebar to open it here.")
                .when(!self.layout.sidebar, |state| {
                    state.action(
                        Button::new("empty-show-sidebar")
                            .small()
                            .icon(Lucide::PanelLeft)
                            .label("Show Sidebar")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.toggle_area(Area::Sidebar, cx)),
                            ),
                    )
                })
                .action(
                    Button::new("empty-open-folder")
                        .small()
                        .icon(Lucide::FolderOpen)
                        .label("Open Folder…")
                        .on_click(|_, window, cx| {
                            crate::cli::prompt_open_folder(Some(window.window_handle()), cx)
                        }),
                )
                .into_any_element(),
        }
    }
}

thread_local! {
    /// Whether the window being drawn has assistive technology attached that walks the whole
    /// accessibility tree (VoiceOver, Switch Control, or an end-to-end run's driver), as the
    /// workspace's render saw it: see [`cached`].
    static A11Y_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The last answer of [`screen_reader_on`] and when it was read, so a render asks macOS at
    /// most once every [`SCREEN_READER_RECHECK`].
    static SCREEN_READER: std::cell::Cell<Option<(std::time::Instant, bool)>> =
        const { std::cell::Cell::new(None) };
}

/// How long a read of whether VoiceOver or Switch Control is on stays good.
const SCREEN_READER_RECHECK: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether the panes must render uncached for the accessibility tree to stay whole. Any app
/// that reads the window's accessibility (a text expander, a grammar checker, a clipboard or
/// launcher utility reading the focused field) turns GPUI's accessibility on, and stays on, but
/// only a screen reader, Switch Control or an end-to-end run walks the whole tree. Rendering
/// uncached for the rest would rebuild every pane on each line of terminal output.
fn needs_whole_tree() -> bool {
    static E2E_RUN: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var_os("SAWIT_E2E_RUN").is_some());
    if *E2E_RUN {
        return true;
    }
    SCREEN_READER.with(|cell| match cell.get() {
        Some((at, on)) if at.elapsed() < SCREEN_READER_RECHECK => on,
        _ => {
            let on = screen_reader_on();
            cell.set(Some((std::time::Instant::now(), on)));
            on
        }
    })
}

/// Whether VoiceOver or Switch Control is on.
#[cfg(target_os = "macos")]
fn screen_reader_on() -> bool {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    workspace.isVoiceOverEnabled() || workspace.isSwitchControlEnabled()
}

/// Elsewhere, any attached assistive technology counts: there is no cheap way to tell a screen
/// reader from other clients.
#[cfg(not(target_os = "macos"))]
fn screen_reader_on() -> bool {
    true
}

/// Mounts a pane so it re-renders only when it notifies, not every time the
/// workspace does (terminal output, a cursor blink or a poll elsewhere
/// would otherwise rebuild the whole window).
///
/// Not while a screen reader, Switch Control or an end-to-end run reads the accessibility tree
/// ([`needs_whole_tree`]): GPUI leaves a cached view's accessibility nodes out of every frame
/// that reuses the view's last render, so the panes' buttons, fields and lists would vanish from
/// the accessibility tree as soon as they stopped changing.
pub(super) fn cached(view: AnyView) -> AnyElement {
    if A11Y_ACTIVE.get() {
        return view.into_any_element();
    }
    view.cached(StyleRefinement::default().size_full())
        .into_any_element()
}
