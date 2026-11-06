//! The app's pages, one tab each: Changelog, MCP, the keymap editor (and the keymap file), Dev Tools
//! and Skills, the last two asking before they close while they install.

use super::*;

impl Workspace {
    /// Shows the version & changelog page (one tab).
    pub(super) fn open_changelog_page(&mut self, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        let key: SharedString = "changelog".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let view = cx.new(ChangelogPage::new);
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Changelog".into(),
                    icon: Lucide::ScrollText,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Shows the MCP install page (one tab), with the tab area, which may be hidden, and the
    /// keyboard in its search.
    pub(super) fn open_mcp_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_mcp_page(true, window, cx);
    }

    /// Shows the MCP page; `focus` gives its search the keyboard (not when an
    /// agent opens it: the keyboard stays where the user has it).
    pub(super) fn show_mcp_page(
        &mut self,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "mcp".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            if focus
                && let PaneItem::Other { view, .. } = &self.tabs[ix].item
                && let Ok(page) = view.clone().downcast::<McpPage>()
            {
                page.update(cx, |page, cx| page.focus_search(window, cx));
            }
        } else {
            let view = cx.new(|cx| McpPage::new(window, cx));
            if focus {
                view.update(cx, |page, cx| page.focus_search(window, cx));
            }
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "MCP".into(),
                    icon: Lucide::Plug,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Shows the keymap editor (one tab), with the tab area, which may be hidden, and the
    /// keyboard.
    pub(super) fn open_keymap_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_keymap_editor(true, window, cx);
    }

    /// Shows the keymap editor; `focus` gives it the keyboard. Not when an agent opens it: the
    /// keyboard stays where the user has it, as for the Dev Tools and MCP pages. There, a ⌫
    /// meant for the terminal would remove the selected binding from keymap.json.
    pub(super) fn show_keymap_editor(
        &mut self,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "keymap".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            if focus
                && let PaneItem::Other { view, .. } = &self.tabs[ix].item
                && let Ok(editor) = view.clone().downcast::<KeymapEditor>()
            {
                window.focus(&editor.focus_handle(cx), cx);
            }
        } else {
            let view = cx.new(|cx| KeymapEditor::new(window, cx));
            if focus {
                window.focus(&view.focus_handle(cx), cx);
            }
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Keymap Editor".into(),
                    icon: Lucide::Keyboard,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Opens the keymap file in an editor tab, writing a new one with a few lines on how to fill
    /// it if there is none.
    pub(super) fn open_keymap_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = crate::keymap::store(cx);
        let path = store.read(cx).path().to_path_buf();
        if let Err(error) = store.read(cx).ensure_file() {
            log::warn!("sawit: can't create {}: {error}", path.display());
            return;
        }
        self.show_center(cx);
        self.open_file(path, window, cx);
    }

    /// Shows the Dev Tools install page (one tab), with the tab area, which may be hidden, and
    /// the keyboard in its search.
    pub(super) fn open_tools_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_tools_page(true, window, cx);
    }

    /// The GitHub CLI, one click away: without `gh`, the Dev Tools page installs it (asking
    /// first when Homebrew is missing too); with it, `gh auth login` runs in a terminal. Sawit's
    /// own GitHub sign-in is never copied into gh.
    pub(super) fn set_up_github_cli(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if sawit_core::dev_tools::find_program("gh").is_some() {
            let _ = self.run_in_terminal("gh auth login", Origin::User, window, cx);
            return;
        }
        self.show_tools_page(false, window, cx);
        let page = self.tabs.iter().find_map(|t| match &t.item {
            PaneItem::Other { view, key, .. } if key.as_ref() == "tools" => {
                view.clone().downcast::<ToolsPage>().ok()
            }
            _ => None,
        });
        if let Some(page) = page {
            page.update(cx, |page, cx| page.install_when_detected("gh", cx));
        }
    }

    /// Shows the Dev Tools page; `focus` gives its search the keyboard (not
    /// when an agent opens it: the keyboard stays where the user has it).
    pub(super) fn show_tools_page(
        &mut self,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "tools".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            // The keyboard goes to its search, as for a new one.
            if focus
                && let PaneItem::Other { view, .. } = &self.tabs[ix].item
                && let Ok(page) = view.clone().downcast::<ToolsPage>()
            {
                page.update(cx, |page, cx| page.focus_search(window, cx));
            }
        } else {
            let view = cx.new(|cx| ToolsPage::new(window, cx));
            if focus {
                view.update(cx, |page, cx| page.focus_search(window, cx));
            }
            cx.subscribe_in(
                &view,
                window,
                |this, _, e: &ToolsEvent, window, cx| match e {
                    ToolsEvent::RunInTerminal(command) => {
                        let _ = this.run_in_terminal(command, Origin::User, window, cx);
                    }
                    ToolsEvent::Launch {
                        title,
                        command,
                        env,
                    } => this.run_in_new_terminal(title, command, env, window, cx),
                },
            )
            .detach();
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Dev Tools".into(),
                    icon: Lucide::Package,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Shows the Skills page (one tab), with the tab area, which may be hidden, and the keyboard
    /// in its search.
    pub(super) fn open_skills_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_skills_page(true, window, cx);
    }

    /// Shows the Permissions page (one tab) for this window's open projects, reading its files
    /// again; `focus` gives it the keyboard (not when an agent opens it).
    pub(super) fn show_permissions_page(
        &mut self,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let projects = self.roots(cx);
        let key: SharedString = "permissions".into();
        let view = if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            let PaneItem::Other { view, .. } = &self.tabs[ix].item else {
                return;
            };
            let Ok(page) = view.clone().downcast::<PermissionsPage>() else {
                return;
            };
            page.update(cx, |page, cx| page.set_projects(projects, cx));
            page
        } else {
            let view = cx.new(|cx| PermissionsPage::new(projects, cx));
            self.push_tab(
                PaneItem::Other {
                    view: view.clone().into(),
                    key,
                    title: "Permissions".into(),
                    icon: Lucide::ShieldCheck,
                    file: None,
                },
                None,
                cx,
            );
            view
        };
        if focus {
            view.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    /// Shows the Skills page; `focus` gives its search the keyboard (not when an agent opens it:
    /// the keyboard stays where the user has it).
    pub(super) fn show_skills_page(
        &mut self,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "skills".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            if focus
                && let PaneItem::Other { view, .. } = &self.tabs[ix].item
                && let Ok(page) = view.clone().downcast::<SkillsPage>()
            {
                page.update(cx, |page, cx| page.focus_search(window, cx));
            }
        } else {
            let view = cx.new(|cx| SkillsPage::new(window, cx));
            if focus {
                view.update(cx, |page, cx| page.focus_search(window, cx));
            }
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Skills".into(),
                    icon: Lucide::Sparkles,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Asks before closing the Dev Tools tab while it installs: closing it
    /// stops the installer, and what's queued doesn't run.
    pub(super) fn confirm_close_tools(
        &mut self,
        work: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = crate::confirm::Confirm::new("Stop the Dev Tools installs?", "Stop and Close")
            .detail(format!(
                "{work}. Closing the tab stops it, and what's queued doesn't run."
            ))
            .ask(window, cx);
        cx.spawn(async move |this, cx| {
            if answer.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this.tabs.iter().position(
                        |t| matches!(&t.item, PaneItem::Other { key, .. } if key.as_ref() == "tools"),
                    ) {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Asks before closing the Skills tab while it installs: closing it stops that.
    pub(super) fn confirm_close_skills(
        &mut self,
        work: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = crate::confirm::Confirm::new("Stop the skill installs?", "Stop and Close")
            .detail(format!(
                "{work}. Closing the tab stops it, and what's queued doesn't run."
            ))
            .ask(window, cx);
        cx.spawn(async move |this, cx| {
            if answer.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this.tabs.iter().position(
                        |t| matches!(&t.item, PaneItem::Other { key, .. } if key.as_ref() == "skills"),
                    ) {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }
}

/// What the Dev Tools page in `item` runs and has queued, when it is that page
/// and it runs something.
pub(super) fn tools_work(item: &PaneItem, cx: &App) -> Option<String> {
    let PaneItem::Other { view, key, .. } = item else {
        return None;
    };
    if key.as_ref() != "tools" {
        return None;
    }
    view.clone().downcast::<ToolsPage>().ok()?.read(cx).work()
}

/// What the Skills page in `item` runs and has queued, when it is that page and it runs
/// something.
pub(super) fn skills_work(item: &PaneItem, cx: &App) -> Option<String> {
    let PaneItem::Other { view, key, .. } = item else {
        return None;
    };
    if key.as_ref() != "skills" {
        return None;
    }
    view.clone().downcast::<SkillsPage>().ok()?.read(cx).work()
}

impl Workspace {
    /// Shows the Accounts page (one tab), for the user (it takes the keyboard).
    pub(super) fn open_accounts_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_accounts_page(window, cx);
        if let Some(PaneItem::Other { view, .. }) = self.tabs.get(self.active).map(|t| &t.item)
            && let Ok(page) = view.clone().downcast::<crate::accounts::AccountsPage>()
        {
            page.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    /// Shows the Accounts page; an agent's open leaves the keyboard where it is.
    pub(super) fn show_accounts_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "accounts".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let view = cx.new(|cx| crate::accounts::AccountsPage::new(window, cx));
            let sub = cx.subscribe_in(
                &view,
                window,
                |this, _, e: &crate::accounts::AccountsEvent, window, cx| {
                    let crate::accounts::AccountsEvent::OpenTerminal {
                        title,
                        command,
                        env,
                    } = e;
                    this.open_terminal_running(title, command, env, window, cx);
                },
            );
            self._subscriptions.push(sub);
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Accounts".into(),
                    icon: Lucide::Users,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// A new terminal tab titled `title` that runs `command` with `env`, shown to the user.
    pub(crate) fn open_terminal_running(
        &mut self,
        title: &str,
        command: &str,
        env: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(Layout::default(), cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        let result = self.terminal.update(cx, |t, cx| {
            t.run_in_new_tab(title, command, env, window, cx)
        });
        if let Err(e) = result {
            log::warn!("couldn't open a terminal: {e}");
        }
        cx.notify();
    }
}
