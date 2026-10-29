//! `sawit-mcp` commands for one window (see `sawit_core::app_control`):
//! what it shows, and the panels, tabs, terminals, scripts, SSH sessions,
//! database tabs and canvas an agent can drive.

use futures::channel::oneshot;
use sawit_core::app_control::{Command, Page, Panel, Store, key_bytes};
use sawit_core::db::ConnProfile;
use serde_json::{Value, json};

use super::*;

/// Terminal lines `terminal_read` returns by default, and at most.
const READ_LINES: usize = 200;
const MAX_READ_LINES: usize = 5_000;

/// A command's answer: now, or once the canvas page replies.
pub enum Answer {
    Now(Value),
    Later(oneshot::Receiver<Result<Value, String>>),
}

fn panel_name(tab: SidebarTab) -> &'static str {
    match tab {
        SidebarTab::Servers => "instances",
        SidebarTab::Files => "files",
        SidebarTab::Git => "git",
        SidebarTab::Database => "database",
        SidebarTab::Run => "run",
        SidebarTab::Ports => "ports",
        SidebarTab::Network => "network",
        SidebarTab::Prompts => "prompts",
        SidebarTab::Ssh => "ssh",
        SidebarTab::ShellConfig => "shell_config",
    }
}

fn sidebar_tab(panel: Panel) -> SidebarTab {
    match panel {
        Panel::Instances => SidebarTab::Servers,
        Panel::Files => SidebarTab::Files,
        Panel::Git => SidebarTab::Git,
        Panel::Database => SidebarTab::Database,
        Panel::Run => SidebarTab::Run,
        Panel::Ports => SidebarTab::Ports,
        Panel::Network => SidebarTab::Network,
        Panel::Prompts => SidebarTab::Prompts,
        Panel::Ssh => SidebarTab::Ssh,
        Panel::ShellConfig => SidebarTab::ShellConfig,
    }
}

impl Workspace {
    /// The window as `sawit-mcp`'s `app_state` shows it.
    pub fn describe(&self, cx: &App) -> Value {
        let tabs: Vec<Value> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let (kind, title, path, dirty) = match &tab.item {
                    PaneItem::Server => {
                        ("instance".to_string(), "Instance".to_string(), None, false)
                    }
                    PaneItem::File(e) => {
                        let e = e.read(cx);
                        ("file".to_string(), e.title(), Some(e.path.clone()), e.dirty)
                    }
                    PaneItem::Other {
                        key, title, file, ..
                    } => (
                        key.split('\u{0}').next().unwrap_or_default().to_string(),
                        title.to_string(),
                        file.clone(),
                        false,
                    ),
                };
                json!({
                    "index": ix,
                    "title": title,
                    "kind": kind,
                    "path": path,
                    "active": ix == self.active,
                    "pinned": tab.pinned,
                    "unsaved": dirty,
                })
            })
            .collect();
        json!({
            "project": self.root(cx),
            "projects": self.roots(cx),
            "welcome_screen": self.welcome.is_some(),
            "panel": panel_name(self.sidebar),
            "layout": {
                "sidebar": self.layout.sidebar,
                "center": self.layout.center,
                "terminal": self.layout.terminal,
            },
            "tabs": tabs,
            "terminals": self.terminal.read(cx).describe(cx),
            "canvas_open": self.canvas.as_ref().is_some_and(|c| c.upgrade().is_some()),
        })
    }

    /// Reads `store` again after `sawit-mcp` changed its file.
    pub fn reload(&mut self, store: Store, cx: &mut Context<Self>) {
        match store {
            Store::Instances => self.servers.update(cx, |s, cx| s.reload(cx)),
            Store::Connections => self.db.update(cx, |s, cx| s.reload(cx)),
            Store::Prompts => self.prompt_store.update(cx, |s, cx| s.reload(cx)),
            Store::Ssh => self.ssh_store.update(cx, |s, cx| s.reload(cx)),
            Store::RunCommands => {
                for p in &self.projects {
                    p.run_panel.update(cx, |r, cx| r.reload_saved(cx));
                }
            }
        }
    }

    /// Runs a window command. `State`, `Focus`, `Open` and `Reload` are
    /// handled for the whole app by the caller.
    pub fn control(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Answer, String> {
        let done = |value: Value| Ok(Answer::Now(value));
        let result = match command {
            Command::State => done(self.describe(cx)),
            Command::Focus | Command::Open { .. } | Command::Reload { .. } => done(json!({})),
            Command::ShowPanel { panel } => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.layout.sidebar = true;
                self.sidebar = sidebar_tab(panel);
                done(json!({ "panel": panel_name(self.sidebar) }))
            }
            Command::Layout {
                sidebar,
                terminal,
                center,
            } => {
                self.leave_welcome(Layout::default(), cx);
                let mut layout = self.layout;
                layout.sidebar = sidebar.unwrap_or(layout.sidebar);
                layout.terminal = terminal.unwrap_or(layout.terminal);
                layout.center = center.unwrap_or(layout.center);
                if !layout.center && !layout.terminal {
                    return Err("The tabs or the terminal must stay visible.".into());
                }
                self.layout = layout;
                self.canvas_maximized = false;
                done(json!({
                    "sidebar": layout.sidebar,
                    "center": layout.center,
                    "terminal": layout.terminal,
                }))
            }
            Command::MaximizeTerminal => {
                self.leave_welcome(Layout::default(), cx);
                self.toggle_terminal_maximized(window, cx);
                done(json!({ "maximized": self.layout == TERMINAL_ONLY }))
            }
            Command::OpenPage { page } => {
                self.open_page(page, window, cx)?;
                done(json!({ "tab": self.active }))
            }
            Command::ActivateTab { tab } => {
                let ix = self.find_tab(&tab, cx)?;
                self.show_center(cx);
                self.active = ix;
                done(json!({ "tab": ix }))
            }
            Command::CloseTab { tab } => {
                let ix = self.find_tab(&tab, cx)?;
                self.close(CloseOp::This, ix, window, cx);
                done(json!({ "closed": ix, "tabs": self.tabs.len() }))
            }
            Command::TerminalRun {
                command,
                cwd,
                new_tab,
            } => {
                let tab = if cwd.is_some() || new_tab {
                    self.leave_welcome(TERMINAL_ONLY, cx);
                    self.layout.terminal = true;
                    let dir = cwd.or_else(|| self.root(cx));
                    self.terminal
                        .update(cx, |t, cx| t.run_in_new(dir, &command, window, cx))?
                } else {
                    self.run_in_terminal(&command, window, cx);
                    self.terminal.read(cx).active_index()
                };
                done(json!({ "terminal": tab }))
            }
            Command::TerminalRead { tab, lines } => {
                let lines = lines.unwrap_or(READ_LINES).clamp(1, MAX_READ_LINES);
                let text = self.terminal.read(cx).text(tab, lines, cx)?;
                done(json!({ "text": text }))
            }
            Command::TerminalInput { tab, text, key } => {
                let mut bytes = text.map(String::into_bytes).unwrap_or_default();
                if let Some(key) = key {
                    bytes.extend(key_bytes(&key).ok_or_else(|| {
                        format!("Unknown key “{key}”. Use enter, tab, escape, backspace, up, down, left, right or ctrl-<letter>.")
                    })?);
                }
                if bytes.is_empty() {
                    return Err("Give text, a key, or both.".into());
                }
                let ix = self.terminal.update(cx, |t, cx| t.input(tab, bytes, cx))?;
                done(json!({ "terminal": ix }))
            }
            Command::TerminalClose { tab } => {
                self.terminal.update(cx, |t, cx| t.close_tab(tab, cx))?;
                done(json!({ "closed": tab }))
            }
            Command::RunList => {
                let scripts = self.current().run_panel.read(cx).describe(cx)?;
                done(json!({ "scripts": scripts }))
            }
            Command::RunScript { name } => {
                self.current()
                    .run_panel
                    .update(cx, |r, cx| r.start_named(&name, cx))?;
                done(json!({ "started": name }))
            }
            Command::StopScript { name } => {
                self.current()
                    .run_panel
                    .update(cx, |r, cx| r.stop_named(&name, cx))?;
                done(json!({ "stopped": name }))
            }
            Command::SshConnect { host, new_session } => {
                let key = self.ssh_key(&host, cx)?;
                self.connect_ssh(key, new_session, window, cx);
                done(json!({ "terminal": self.terminal.read(cx).active_index() }))
            }
            Command::OpenTable {
                connection,
                schema,
                table,
            } => {
                let profile = self.db_profile(&connection, cx)?;
                self.open_db(
                    DbEvent::OpenTable {
                        profile,
                        schema,
                        table,
                    },
                    window,
                    cx,
                )
            }
            Command::OpenSql { connection, sql } => {
                let profile = self.db_profile(&connection, cx)?;
                self.open_db(DbEvent::OpenSql { profile, sql }, window, cx)
            }
            Command::OpenDiagram { connection, schema } => {
                let profile = self.db_profile(&connection, cx)?;
                self.open_db(DbEvent::OpenDiagram { profile, schema }, window, cx)
            }
            Command::OpenPrompt { id } => {
                if self.prompt_store.read(cx).library.get(&id).is_none() {
                    return Err(format!("No prompt has id “{id}”. See list_prompts."));
                }
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.open_prompt(id, window, cx);
                done(json!({ "tab": self.active }))
            }
            Command::Canvas { op } => {
                self.open_canvas(cx);
                let canvas = self
                    .canvas
                    .as_ref()
                    .and_then(|c| c.upgrade())
                    .ok_or("The canvas didn't open.")?;
                Ok(Answer::Later(canvas.update(cx, |c, _| c.apply(op))))
            }
        };
        cx.notify();
        result
    }

    fn open_page(
        &mut self,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match page {
            Page::Canvas => self.open_canvas(cx),
            Page::Mcp => self.open_mcp_page(cx),
            Page::Changelog => {
                self.show_center(cx);
                self.open_changelog_page(cx)
            }
            Page::GitGraph | Page::Github => {
                if self.root(cx).is_none() {
                    return Err("Open a project folder first.".into());
                }
                self.show_center(cx);
                match page {
                    Page::GitGraph => self.open_git_graph(cx),
                    _ => self.open_github(window, cx),
                }
            }
            Page::Instance => {
                self.show_center(cx);
                self.show_server_tab();
            }
        }
        Ok(())
    }

    /// A center tab by index (from `state`) or title: exact (any case),
    /// else the only title containing it.
    fn find_tab(&self, tab: &str, cx: &App) -> Result<usize, String> {
        let titles: Vec<String> = self
            .tabs
            .iter()
            .map(|t| match &t.item {
                PaneItem::Server => "Instance".to_string(),
                PaneItem::File(e) => e.read(cx).title(),
                PaneItem::Other { title, .. } => title.to_string(),
            })
            .collect();
        if let Ok(ix) = tab.trim().parse::<usize>() {
            return match ix < titles.len() {
                true => Ok(ix),
                false => Err(format!("There is no tab {ix}; {} are open.", titles.len())),
            };
        }
        let wanted = tab.to_lowercase();
        if let Some(ix) = titles.iter().position(|t| t.to_lowercase() == wanted) {
            return Ok(ix);
        }
        let partial: Vec<usize> = (0..titles.len())
            .filter(|&ix| titles[ix].to_lowercase().contains(&wanted))
            .collect();
        match partial.as_slice() {
            [ix] => Ok(*ix),
            [] => Err(format!(
                "No tab is called “{tab}”. Open tabs: {}.",
                titles.join(", ")
            )),
            _ => Err(format!(
                "Several tabs match “{tab}”: {}. Use the index.",
                partial
                    .iter()
                    .map(|&ix| format!("{ix} {}", titles[ix]))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// The store key of an SSH server: a saved one by id or name, else a
    /// `~/.ssh/config` alias.
    fn ssh_key(&self, host: &str, cx: &App) -> Result<String, String> {
        let store = self.ssh_store.read(cx);
        if let Some(h) = store.list.get(host) {
            return Ok(crate::ssh::saved_key(&h.id));
        }
        let named: Vec<_> = store
            .list
            .hosts
            .iter()
            .filter(|h| h.name == host || h.label() == host)
            .collect();
        match named.as_slice() {
            [h] => return Ok(crate::ssh::saved_key(&h.id)),
            [] => {}
            _ => {
                return Err(format!(
                    "Several saved servers are called “{host}”. Use the id from list_ssh_hosts."
                ));
            }
        }
        if store.config.iter().any(|c| c.alias == host) {
            return Ok(crate::ssh::alias_key(host));
        }
        Err(format!(
            "“{host}” isn't a saved server or a ~/.ssh/config host. See list_ssh_hosts."
        ))
    }

    /// The Database panel's connection for `name`: a saved connection, else
    /// a Sawit instance (added as “name (Sawit)”, like Connect Instances).
    fn db_profile(&mut self, name: &str, cx: &mut Context<Self>) -> Result<String, String> {
        let store = self.db.read(cx);
        if store.profile(name).is_some() {
            return Ok(name.to_string());
        }
        let as_instance = format!("{name} (Sawit)");
        if store.profile(&as_instance).is_some() {
            return Ok(as_instance);
        }
        let port = self
            .servers
            .read(cx)
            .registry
            .get(name)
            .map(|i| i.port)
            .ok_or_else(|| {
                format!(
                    "“{name}” isn't a saved connection or a Sawit instance. See list_connections."
                )
            })?;
        let profile = ConnProfile {
            name: as_instance.clone(),
            ..ConnProfile::new("localhost", port, "postgres", "postgres")
        };
        self.db.update(cx, |s, cx| s.add_profile(profile, cx));
        Ok(as_instance)
    }

    /// Opens a table, SQL or diagram tab; it appears once connected.
    fn open_db(
        &mut self,
        event: DbEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Answer, String> {
        self.show_center(cx);
        self.layout.sidebar = true;
        self.sidebar = SidebarTab::Database;
        self.on_db_event(&event, window, cx);
        let waiting = self.pending_db.is_some();
        Ok(Answer::Now(json!({
            "tab": (!waiting).then_some(self.active),
            "connecting": waiting,
        })))
    }
}
