//! A window's workspace as it starts: its stores and panels, and what it does on their events.

use super::*;

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let servers = cx.new(ServerStore::new);
        let terminal = cx.new(TerminalPanel::new);
        let db = cx.new(DbStore::new);
        let db_panel = cx.new(|cx| DatabasePanel::new(db.clone(), servers.clone(), window, cx));
        let prompt_store = cx.new(PromptStore::new);
        let prompts_panel = cx.new(|cx| PromptsPanel::new(prompt_store.clone(), window, cx));
        let ports_panel = cx.new(|cx| PortsPanel::new(servers.clone(), window, cx));
        let run_store = cx.new(RunStore::new);
        let project = Self::new_project(&run_store, &terminal, &ports_panel, window, cx);
        let network_panel = cx.new(|cx| NetworkPanel::new(window, cx));
        let ssh_store = cx.new(SshStore::new);
        let ssh_panel = cx.new(|cx| SshPanel::new(ssh_store.clone(), terminal.clone(), window, cx));
        // Closing the window ends its terminals: ask while one runs something.
        crate::terminal::ask_before_closing(window, terminal.downgrade(), cx);
        // SSH sessions reconnect with their server's settings as they are now.
        let saved_servers = ssh_store.downgrade();
        terminal.update(cx, |t, _| {
            t.set_remote_source(move |key, cx| {
                let store = saved_servers.upgrade().ok_or("Sawit is closing.")?;
                store.update(cx, |s, cx| s.remote(key, cx))
            })
        });
        let shell_config_panel = cx.new(|cx| ShellConfigPanel::new(window, cx));
        let cli_status = cli_status(cx);
        let updater = updater(cx);
        let github = crate::git::account::global(cx);
        let welcome = cx.new(|cx| Welcome::new(servers.clone(), db.clone(), cx));
        let focus = cx.focus_handle();
        let keyboard = KeyboardHome::new(focus.clone(), window, cx);
        let mut subscriptions = vec![
            cx.observe(&servers, |_, _, cx| cx.notify()),
            cx.observe(&cli_status, |_, _, cx| cx.notify()),
            cx.observe(&updater, |_, _, cx| cx.notify()),
            cx.observe(&github, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &welcome,
                window,
                |this, _, event: &WelcomeEvent, window, cx| {
                    this.on_welcome_event(event, window, cx)
                },
            ),
            cx.subscribe_in(
                &ports_panel,
                window,
                |this, _, event: &PortsEvent, window, cx| match event {
                    PortsEvent::OpenInBrowser(url) => {
                        this.open_browser(Some(url.clone()), window, cx);
                    }
                },
            ),
            cx.subscribe(&servers, |this, _, _: &ShowServerTab, cx| {
                this.show_server_tab();
                cx.notify();
            }),
            cx.subscribe_in(
                &terminal,
                window,
                |this, _, _: &ToggleMaximize, window, cx| {
                    this.toggle_terminal_maximized(Origin::User, window, cx)
                },
            ),
            cx.observe_in(&db, window, |this, db, window, cx| {
                let ready = match &this.pending_db {
                    Some(event) => {
                        let profile = event.profile();
                        let store = db.read(cx);
                        if store.state(profile).is_pending() {
                            return;
                        }
                        store.is_connected(profile)
                    }
                    None => return,
                };
                // Replay once connected; drop it if reconnecting failed.
                if let Some(event) = this.pending_db.take()
                    && ready
                {
                    this.on_db_event(&event, window, cx);
                }
            }),
            cx.subscribe_in(&db_panel, window, |this, _, event: &DbEvent, window, cx| {
                this.on_db_event(event, window, cx)
            }),
            cx.subscribe_in(
                &prompts_panel,
                window,
                |this, _, PromptsEvent::Open(id), window, cx| {
                    this.open_prompt(id.clone(), window, cx)
                },
            ),
            // Prompt tabs follow renames and close when deleted.
            cx.observe(&prompt_store, |this, store, cx| {
                let library = &store.read(cx).library;
                for tab in &mut this.tabs {
                    if let PaneItem::Other { key, title, .. } = &mut tab.item
                        && let Some(id) = key.strip_prefix("prompt\u{0}")
                        && let Some(p) = library.get(id)
                    {
                        let name = if p.title.trim().is_empty() {
                            "(untitled)"
                        } else {
                            p.title.as_str()
                        };
                        if title.as_ref() != name {
                            *title = name.to_string().into();
                        }
                    }
                }
                cx.notify();
            }),
            cx.subscribe_in(
                &ssh_panel,
                window,
                |this, _, event: &SshEvent, window, cx| match event {
                    SshEvent::Connect { key, new } => {
                        let _ = this.connect_ssh(key.clone(), *new, Origin::User, window, cx);
                    }
                    SshEvent::Edit(id) => this.open_ssh_host(id.clone(), window, cx),
                    SshEvent::OpenFile(path) => {
                        this.show_center(cx);
                        this.open_file(path.clone(), window, cx)
                    }
                },
            ),
            cx.subscribe_in(
                &shell_config_panel,
                window,
                |this, _, event: &ShellConfigEvent, window, cx| match event {
                    ShellConfigEvent::OpenFile(path) => {
                        this.show_center(cx);
                        this.open_file(path.clone(), window, cx)
                    }
                    ShellConfigEvent::OpenTerminal(dir) => {
                        this.open_terminal_in(dir.clone(), window, cx)
                    }
                },
            ),
            // Server tabs follow renames and close when deleted.
            cx.observe(&ssh_store, |this, store, cx| {
                let list = &store.read(cx).list;
                for tab in &mut this.tabs {
                    if let PaneItem::Other { key, title, .. } = &mut tab.item
                        && let Some(id) = key.strip_prefix("ssh-host\u{0}")
                        && let Some(h) = list.get(id)
                    {
                        let name = h.label();
                        if title.as_ref() != name {
                            *title = name.into();
                        }
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&ssh_store, |this, _, event: &SshStoreEvent, cx| {
                let SshStoreEvent::Removed(id) = event;
                let key = format!("ssh-host\u{0}{id}");
                if let Some(ix) = this.tabs.iter().position(
                    |t| matches!(&t.item, PaneItem::Other { key: k, .. } if k.as_ref() == key),
                ) {
                    this.remove_tabs(&[ix], cx);
                }
            }),
            cx.subscribe(&prompt_store, |this, _, event: &PromptStoreEvent, cx| {
                let PromptStoreEvent::Removed(id) = event;
                let key = format!("prompt\u{0}{id}");
                if let Some(ix) = this.tabs.iter().position(
                    |t| matches!(&t.item, PaneItem::Other { key: k, .. } if k.as_ref() == key),
                ) {
                    this.remove_tabs(&[ix], cx);
                }
            }),
        ];
        // The footer's Sawit Skills button follows every detection and run.
        if let Some(sawit_skills) = crate::sawit_skills::try_status(cx) {
            subscriptions.push(cx.observe(&sawit_skills, |_, _, cx| cx.notify()));
        }
        Self {
            server_list: cx.new(|cx| ServerList::new(servers.clone(), cx)),
            server_detail: cx.new(|cx| ServerDetail::new(servers.clone(), window, cx)),
            servers,
            projects: vec![project],
            project: 0,
            run_store,
            db,
            db_panel,
            prompt_store,
            prompts_panel,
            ports_panel,
            network_panel,
            ssh_store,
            ssh_panel,
            shell_config_panel,
            sql_count: 0,
            run_next_sql: false,
            terminal,
            sidebar: SidebarTab::Servers,
            tabs: Vec::new(),
            active: 0,
            collapsed: HashSet::new(),
            layout: Layout::default(),
            path_copied: false,
            _path_copied_reset: Task::ready(()),
            unmaximized: None,
            boards: Vec::new(),
            canvas_maximized: false,
            browsers: Vec::new(),
            welcome: Some(welcome),
            tab_scroll: ScrollHandle::new(),
            scrolled_to: None,
            pending_db: None,
            focus,
            keyboard,
            cli_status,
            updater,
            github,
            _subscriptions: subscriptions,
        }
    }
}
