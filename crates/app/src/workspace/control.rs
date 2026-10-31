//! `sawit-mcp` commands for one window (see `sawit_core::app_control`):
//! what it shows, and the panels, tabs, terminals, scripts, SSH sessions,
//! database tabs and canvas an agent can drive.

use futures::channel::oneshot;
use sawit_core::app_control::{Command, InstanceAction, Page, Panel, Store, key_bytes};
use sawit_core::control::Preflight;
use sawit_core::db::ConnProfile;
use serde_json::{Value, json};

use super::*;

/// Terminal lines `terminal_read` returns by default, and at most.
const READ_LINES: usize = 200;
const MAX_READ_LINES: usize = 5_000;

/// A command's answer: now, or once the browser page replies.
pub enum Answer {
    Now(Value),
    Later(oneshot::Receiver<Result<Value, String>>),
}

/// Where an agent's browser op goes.
#[derive(Debug, PartialEq)]
enum BrowserRoute {
    /// To the project's browser as it is.
    Use,
    /// To the project's browser, brought forward (or opened).
    Show,
    /// Nowhere: the project has no browser.
    Closed,
}

/// Only opening a page, a screenshot or a device size brings the browser forward (`shows_tab`);
/// other ops work with it in the background, and with none open they say so rather than open
/// one over what the user is looking at.
fn browser_route(open: bool, shown: bool, shows_tab: bool) -> BrowserRoute {
    match (open, shows_tab) {
        (true, false) => BrowserRoute::Use,
        (true, true) if shown => BrowserRoute::Use,
        (_, true) => BrowserRoute::Show,
        (false, false) => BrowserRoute::Closed,
    }
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

/// `app_run_script`'s answer: whether the script started or ran already, and the terminal (tab
/// and pane) it runs in, or each terminal of a saved command that runs several, by name. The
/// agent's `terminal_read` and `terminal_input` need them: the script doesn't come in front of a
/// terminal the user types in.
fn script_answer(name: &str, running: bool, at: &[(String, usize, usize)]) -> Value {
    let mut answer = match running {
        false => json!({ "started": name }),
        true => json!({
            "running": name,
            "note": "It was running already, so it wasn't started again. app_stop_script stops it.",
        }),
    };
    match at {
        [(_, tab, pane)] => {
            answer["terminal"] = json!(tab);
            answer["pane"] = json!(pane);
        }
        _ => {
            answer["terminals"] = at
                .iter()
                .map(|(name, tab, pane)| json!({ "name": name, "terminal": tab, "pane": pane }))
                .collect();
        }
    }
    answer
}

/// `app_close_tab`'s answer for the tab at `ix`, with `tabs` open now: closed, or, when the close
/// asked the user first (`asking`: what about), that the question is up. The tab then closes only
/// if they agree, as when they close it themselves.
fn close_answer(ix: usize, tabs: usize, asking: Option<&str>) -> Value {
    match asking {
        None => json!({ "closed": ix, "tabs": tabs }),
        Some(why) => json!({
            "asking": ix,
            "tabs": tabs,
            "note": format!(
                "Not closed yet: {why}, so Sawit asked the user whether to close it. It closes if \
                 they agree; app_state shows whether it did."
            ),
        }),
    }
}

/// The folder a shell an agent starts goes to, `cwd`, if it is one: an absolute path to a folder
/// that is there. The PTY ignores a `chdir` that fails, so anything else would start the shell in
/// the app's own folder (`/` for Sawit.app), and the agent's command would run there (MCP-4).
fn shell_folder(cwd: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    match cwd {
        Some(dir) if !dir.is_absolute() => Err(format!(
            "{} isn't an absolute path: give the folder to run in as one.",
            dir.display()
        )),
        Some(dir) if !dir.is_dir() => Err(format!(
            "{} isn't a folder: nothing was run.",
            dir.display()
        )),
        cwd => Ok(cwd),
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
            "canvas_open": self.boards.iter().any(|b| b.upgrade().is_some()),
            "browser": self
                .browser_tab(self.root(cx).as_deref())
                .map(|(_, b)| b.read(cx).describe()),
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
                if self.sidebar == SidebarTab::ShellConfig {
                    let roots = self.roots(cx);
                    self.shell_config_panel
                        .update(cx, |p, cx| p.set_projects(roots, cx));
                }
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
                self.toggle_terminal_maximized(Origin::Agent, window, cx);
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
                let asking = self.close(CloseOp::This, ix, window, cx);
                done(close_answer(ix, self.tabs.len(), asking))
            }
            Command::TerminalRun {
                command,
                cwd,
                new_tab,
            } => {
                let cwd = shell_folder(cwd)?;
                let (tab, pane) = if cwd.is_some() || new_tab {
                    self.leave_welcome(TERMINAL_ONLY, cx);
                    self.layout.terminal = true;
                    let dir = cwd.or_else(|| self.root(cx));
                    self.terminal.update(cx, |t, cx| {
                        t.run_in_new(dir, &command, Origin::Agent, window, cx)
                    })?
                } else {
                    self.run_in_terminal(&command, Origin::Agent, window, cx)?
                };
                let pane = self.terminal.read(cx).pane_index(tab, pane, cx);
                done(json!({ "terminal": tab, "pane": pane }))
            }
            Command::TerminalRead { tab, pane, lines } => {
                let lines = lines.unwrap_or(READ_LINES).clamp(1, MAX_READ_LINES);
                let text = self.terminal.read(cx).text(tab, pane, lines, cx)?;
                done(json!({ "text": text }))
            }
            Command::TerminalInput {
                tab,
                pane,
                text,
                key,
            } => {
                let mut bytes = text.map(String::into_bytes).unwrap_or_default();
                if let Some(key) = key {
                    bytes.extend(key_bytes(&key).ok_or_else(|| {
                        format!("Unknown key “{key}”. Use enter, tab, escape, backspace, up, down, left, right or ctrl-<letter>.")
                    })?);
                }
                if bytes.is_empty() {
                    return Err("Give text, a key, or both.".into());
                }
                let ix = self
                    .terminal
                    .update(cx, |t, cx| t.input(tab, pane, bytes, cx))?;
                done(json!({ "terminal": ix }))
            }
            Command::TerminalClose { tab, pane } => {
                self.terminal
                    .update(cx, |t, cx| t.close_tab(tab, pane, window, cx))?;
                done(json!({ "closed": tab }))
            }
            Command::RunList => {
                let scripts = self.current().run_panel.read(cx).describe(cx)?;
                done(json!({ "scripts": scripts }))
            }
            Command::RunScript { name } => {
                let (launches, running) = self
                    .current()
                    .run_panel
                    .read(cx)
                    .launches_named(&name, cx)?;
                self.start_scripts(&launches, Origin::Agent, window, cx)
                    .map_err(|e| format!("“{name}” didn't start: {e}"))?;
                let terminal = self.terminal.read(cx);
                let at: Vec<(String, usize, usize)> = launches
                    .iter()
                    .filter_map(|l| {
                        let (tab, pane) = terminal.job_terminal(&l.tag.key, cx)?;
                        Some((l.tag.name.clone(), tab, pane))
                    })
                    .collect();
                done(script_answer(&name, running, &at))
            }
            Command::StopScript { name } => {
                self.current()
                    .run_panel
                    .update(cx, |r, cx| r.stop_named(&name, cx))?;
                done(json!({ "stopped": name }))
            }
            Command::SshConnect { host, new_session } => {
                let key = self.ssh_key(&host, cx)?;
                let (tab, pane) = self.connect_ssh(key, new_session, Origin::Agent, window, cx)?;
                let pane = self.terminal.read(cx).pane_index(tab, pane, cx);
                done(json!({ "terminal": tab, "pane": pane }))
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
            Command::OpenSql {
                connection,
                sql,
                run,
            } => {
                let profile = self.db_profile(&connection, cx)?;
                self.run_next_sql = run;
                self.open_db(DbEvent::OpenSql { profile, sql }, window, cx)
            }
            Command::Instance { name, action } => {
                if self.servers.read(cx).registry.get(&name).is_none() {
                    return Err(format!(
                        "No instance is called “{name}”. See list_instances."
                    ));
                }
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.layout.sidebar = true;
                self.sidebar = SidebarTab::Servers;
                self.servers.update(cx, |s, cx| {
                    s.select(&name, cx);
                    match action {
                        InstanceAction::Select => {}
                        InstanceAction::Start => s.start(&name, Preflight::Check, cx),
                        InstanceAction::Stop => s.stop(&name, cx),
                        InstanceAction::Restart => s.restart(&name, cx),
                    }
                });
                self.show_center(cx);
                self.show_server_tab();
                done(json!({
                    "instance": name,
                    "action": action,
                    "note": "Runs in the app. A port conflict holds the start and is explained on the Instance tab; read the outcome with list_instances or instance_logs.",
                }))
            }
            Command::NetworkDiagnose { ping } => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.layout.sidebar = true;
                self.sidebar = SidebarTab::Network;
                self.network_panel.update(cx, |n, cx| {
                    n.diagnose(cx);
                    if let Some(host) = ping.as_deref().filter(|h| !h.trim().is_empty()) {
                        n.ping(host.trim().to_string(), cx);
                    }
                });
                done(json!({
                    "diagnosing": true,
                    "ping": ping,
                    "note": "The checks fill in on the Network tab; diagnose_network returns the same verdict as text.",
                }))
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
            // The canvas answers at once, with the op's own reply: `{ok: false, error}` when it
            // failed.
            Command::Canvas { op } => {
                let board = self.open_canvas(cx).ok_or("The canvas didn't open.")?;
                Ok(Answer::Now(board.update(cx, |b, cx| b.run_op(&op, cx))))
            }
            // The active project's browser: the one for the project the agent named.
            Command::Browser { op } => {
                let open = self.browser_tab(self.root(cx).as_deref()).map(|(_, b)| b);
                let shown = open.is_some() && open == self.active_browser();
                let browser = match (browser_route(open.is_some(), shown, op.shows_tab()), open) {
                    (BrowserRoute::Use, Some(browser)) => browser,
                    (BrowserRoute::Closed, _) => {
                        return Err(sawit_core::browser::NOT_OPEN.into());
                    }
                    _ => self.open_browser(None, window, cx),
                };
                Ok(Answer::Later(browser.update(cx, |b, cx| b.apply(op, cx))))
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
            Page::Canvas => {
                self.open_canvas(cx);
            }
            Page::Browser => {
                self.open_browser(None, window, cx);
            }
            // An agent's: the keyboard stays where the user has it.
            Page::Mcp => self.show_mcp_page(false, window, cx),
            Page::Tools => self.show_tools_page(false, window, cx),
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
            Page::Keymap => {
                self.show_center(cx);
                self.open_keymap_editor(window, cx)
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::{BrowserRoute, browser_route, close_answer, script_answer, shell_folder};

    /// MCP-4: a shell an agent starts in a folder starts there or not at all. The PTY ignores a
    /// `chdir` that fails, so a relative folder or one that isn't there started the shell in the
    /// app's own folder (`/` for Sawit.app), and the agent's command ran there.
    #[test]
    fn a_shell_starts_in_a_folder_that_is_there_or_not_at_all() {
        let here = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(shell_folder(None), Ok(None));
        assert_eq!(shell_folder(Some(here.clone())), Ok(Some(here)));
        for (dir, why) in [
            ("web", "isn't an absolute path"),
            ("/no/such/folder", "isn't a folder"),
        ] {
            let refused = shell_folder(Some(PathBuf::from(dir))).unwrap_err();
            assert!(refused.contains(why), "{dir}: {refused}");
        }
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(shell_folder(Some(file)).is_err());
    }

    /// app_run_script names the terminal its script runs in, or each terminal of a saved command
    /// that runs several: the agent's terminal_read and terminal_input need it, as the terminal in
    /// front may be the one the user types in.
    #[test]
    fn a_scripts_answer_says_where_it_runs() {
        assert_eq!(
            script_answer("dev", false, &[("dev".into(), 2, 0)]),
            json!({ "started": "dev", "terminal": 2, "pane": 0 })
        );
        assert_eq!(
            script_answer("dev", true, &[("dev".into(), 2, 1)]),
            json!({
                "running": "dev",
                "note": "It was running already, so it wasn't started again. app_stop_script stops it.",
                "terminal": 2,
                "pane": 1,
            })
        );
        let both = [("both 1".to_string(), 3, 0), ("both 2".to_string(), 4, 0)];
        assert_eq!(
            script_answer("both", false, &both),
            json!({
                "started": "both",
                "terminals": [
                    { "name": "both 1", "terminal": 3, "pane": 0 },
                    { "name": "both 2", "terminal": 4, "pane": 0 },
                ],
            })
        );
    }

    /// app_close_tab says "closed" only when the tab closed. A tab that would lose something (a
    /// file's unsaved changes, the browser's notes not yet sent, a Dev Tools install) asks the
    /// user first and closes only if they agree, so its answer says a question is up: it said
    /// "closed" before the user had answered.
    #[test]
    fn closing_a_tab_says_when_the_user_is_asked_first() {
        assert_eq!(close_answer(2, 4, None), json!({ "closed": 2, "tabs": 4 }));
        let asked = close_answer(2, 5, Some("the file has unsaved changes"));
        assert_eq!(asked.get("closed"), None, "{asked}");
        assert_eq!(asked["asking"], json!(2), "{asked}");
        assert_eq!(asked["tabs"], json!(5), "{asked}");
        let note = asked["note"].as_str().unwrap_or_default();
        assert!(note.contains("the file has unsaved changes"), "{asked}");
        assert!(note.contains("app_state"), "{asked}");
    }

    /// An agent's `browser_console` or `browser_annotations` with no browser open opened one
    /// over the user's tab, to say "No annotations yet"; only ops that show a page open it.
    #[test]
    fn only_ops_that_show_a_page_open_the_browser() {
        use BrowserRoute::{Closed, Show, Use};
        // open, shown, shows_tab
        assert_eq!(browser_route(true, false, false), Use, "in the background");
        assert_eq!(browser_route(true, true, true), Use);
        assert_eq!(browser_route(true, false, true), Show, "brought forward");
        assert_eq!(browser_route(false, false, true), Show, "opened");
        assert_eq!(browser_route(false, false, false), Closed);
    }
}
