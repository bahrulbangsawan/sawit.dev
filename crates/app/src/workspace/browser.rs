//! Browser tabs, one per project (with tabs of its own inside, [`BrowserTabs`]): opening one, sending its annotation notes to an agent, and keeping
//! each native webview in step with what is on screen.

use super::*;

impl Workspace {
    /// Opens the active project's browser (one tab per project, in its group, as its canvas;
    /// one for no project), at `url` when given; an open browser becomes the active tab, and
    /// goes to `url` when one is given. Each project's browser keeps its own cookies, and
    /// writes its screenshots and exports into that project.
    pub(crate) fn open_browser(
        &mut self,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        let root = self.root(cx);
        self.open_browser_of(root, url, window, cx)
    }

    /// Opens the browser of the project at `root` (`None`: the one for no project), as
    /// [`Self::open_browser`] opens the active project's, with its project, as a click on its
    /// tab shows it: an agent's browser is the browser of the project it works in, which may not
    /// be the one shown.
    pub(crate) fn open_browser_of(
        &mut self,
        root: Option<PathBuf>,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        self.leave_welcome(CENTER_ONLY, cx);
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        let browser = match self.browser_tabs_of(root.as_deref()) {
            Some((ix, tabs)) => {
                self.activate_tab(ix, cx);
                let browser = tabs.read(cx).active_view();
                browser.update(cx, |b, cx| b.show(url.as_deref(), cx));
                browser
            }
            None => {
                let tabs = cx.new(|cx| BrowserTabs::new(root.clone(), url, window, cx));
                self.browsers.retain(|b| b.upgrade().is_some());
                self.browsers.push(tabs.downgrade());
                for view in tabs.read(cx).views() {
                    self.wire_browser(&view, window, cx);
                }
                cx.subscribe_in(&tabs, window, |this, tabs, event, window, cx| match event {
                    TabsEvent::Added(view) => this.wire_browser(view, window, cx),
                    TabsEvent::Emptied => {
                        if let Some(ix) = this.tabs.iter().position(|t| {
                            matches!(&t.item, PaneItem::Other { view, .. }
                                if view.clone().downcast::<BrowserTabs>().ok().as_ref() == Some(tabs))
                        }) {
                            this.remove_tabs(&[ix], cx);
                        }
                    }
                })
                .detach();
                self.push_tab(
                    PaneItem::Other {
                        view: tabs.clone().into(),
                        key: browser_key(root.as_deref()),
                        title: crate::browser::TITLE.into(),
                        icon: crate::browser::ICON,
                        file: None,
                    },
                    root,
                    cx,
                );
                tabs.read(cx).active_view()
            }
        };
        cx.notify();
        browser
    }

    /// Hears a browser tab's notes being sent, and tells it which agents run.
    fn wire_browser(
        &mut self,
        view: &Entity<BrowserView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            view,
            window,
            |this, browser, event, window, cx| match event {
                crate::browser::BrowserEvent::SendToAgent { markdown, agent } => {
                    this.send_to_agent(browser, markdown, agent, window, cx)
                }
                crate::browser::BrowserEvent::OpenAgent { markdown, kind } => {
                    this.open_agent(browser, markdown, *kind, window, cx)
                }
                // The browser's tabs open it ([`BrowserTabs`]).
                crate::browser::BrowserEvent::OpenTab { .. } => {}
            },
        )
        .detach();
        let terminal = self.terminal.downgrade();
        let source: crate::browser::AgentSource = Rc::new(move |cx: &App| {
            terminal
                .upgrade()
                .map(|t| t.read(cx).local_agents(cx))
                .unwrap_or_default()
        });
        view.update(cx, |b, cx| b.set_agent_source(source, cx));
    }

    /// The browser tab of the project at `root` (`None`: the one for no project), and the tab on
    /// show in it.
    pub(super) fn browser_tab(
        &self,
        root: Option<&Path>,
        cx: &App,
    ) -> Option<(usize, Entity<BrowserView>)> {
        self.browser_tabs_of(root)
            .map(|(ix, tabs)| (ix, tabs.read(cx).active_view()))
    }

    /// The browser tab of the project at `root`, and its tabs.
    pub(super) fn browser_tabs_of(
        &self,
        root: Option<&Path>,
    ) -> Option<(usize, Entity<BrowserTabs>)> {
        let key = browser_key(root);
        self.tabs
            .iter()
            .enumerate()
            .find_map(|(ix, t)| match &t.item {
                PaneItem::Other { key: k, view, .. } if *k == key => {
                    view.clone().downcast::<BrowserTabs>().ok().map(|b| (ix, b))
                }
                _ => None,
            })
    }

    /// The browser whose tab is the active one, and its tabs.
    pub(super) fn active_browser_tabs(&self) -> Option<Entity<BrowserTabs>> {
        match self.tabs.get(self.active).map(|t| &t.item) {
            Some(PaneItem::Other { view, .. }) => view.clone().downcast::<BrowserTabs>().ok(),
            _ => None,
        }
    }

    /// Sends the browser's annotation notes to `agent`'s chat in its terminal pane
    /// ([`TerminalPanel::send_to_agent`]), showing the terminal if it was hidden. Only that agent
    /// gets them: one that has ended since the panel offered it gets nothing, and the panel says
    /// so. A plain shell never gets them, as its prompt would run them as commands.
    fn send_to_agent(
        &mut self,
        browser: &Entity<BrowserView>,
        markdown: &str,
        agent: &crate::terminal::RunningAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sent = self.terminal.update(cx, |t, cx| {
            t.send_to_agent(agent.pane, agent.kind, markdown, window, cx)
        });
        if sent.is_ok() && !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        let result = sent.map(|a| format!("Sent to {} ({})", a.kind.name(), a.label));
        browser.update(cx, |b, cx| b.finish_send(result, cx));
        cx.notify();
    }

    /// Starts `kind` in a new terminal tab in the browser's project, with the notes as its
    /// first message: they are written to a file beside the day's screenshots, which the shell
    /// reads into the command line ([`crate::browser::open_command`]). The new terminal takes
    /// the keyboard, as the agent may ask something first (whether to trust the folder).
    fn open_agent(
        &mut self,
        browser: &Entity<BrowserView>,
        markdown: &str,
        kind: sawit_core::terminal::AgentKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = browser.read(cx).project().map(Path::to_path_buf);
        let markdown = markdown.to_string();
        let dir = project.clone();
        let written = crate::rt::spawn_blocking(cx, move || {
            crate::browser::write_prompt(dir.as_deref(), &markdown)
        });
        let browser = browser.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let file = written
                .await
                .unwrap_or_else(|_| Err("Couldn't write the notes for the agent.".to_string()));
            let _ = this.update_in(cx, |this, window, cx| {
                let result = file.and_then(|file| {
                    let command = crate::browser::open_command(kind, &file).ok_or_else(|| {
                        format!("Sawit can't start {} with a prompt.", kind.name())
                    })?;
                    if !this.layout.terminal {
                        this.layout.toggle(Area::Terminal);
                    }
                    this.terminal.update(cx, |t, cx| {
                        t.run_in_new(project, &command, Origin::User, window, cx)
                    })?;
                    Ok(format!(
                        "Opened {} in a new terminal with the notes",
                        kind.name()
                    ))
                });
                if let Some(browser) = browser.upgrade() {
                    browser.update(cx, |b, cx| b.finish_send(result, cx));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Keeps the browsers' native webviews in step with what is on screen: they draw above GPUI,
    /// so each is hidden unless its browser's tab is showing and it is that browser's tab on show.
    pub(super) fn sync_browser(&mut self, cx: &mut Context<Self>) {
        self.browsers.retain(|b| b.upgrade().is_some());
        let shown = (self.welcome.is_none() && self.layout.center)
            .then(|| self.active_browser_tabs())
            .flatten();
        let browsers: Vec<Entity<BrowserTabs>> =
            self.browsers.iter().filter_map(|b| b.upgrade()).collect();
        // Hidden first: a webview that hides hands the keyboard back.
        for browser in browsers.iter().filter(|b| shown.as_ref() != Some(*b)) {
            browser.update(cx, |b, cx| b.set_on_show(false, cx));
        }
        if let Some(browser) = shown {
            browser.update(cx, |b, cx| b.set_on_show(true, cx));
        }
    }
}

/// The key of the Browser tab of the project at `root` (`None`: the one for no project).
pub(super) fn browser_key(root: Option<&Path>) -> SharedString {
    match root {
        Some(root) => format!("browser\u{0}{}", root.display()).into(),
        None => "browser".into(),
    }
}
