//! `sawit-mcp` commands for one window (see `sawit_core::app_control`):
//! what it shows, and the panels, tabs, terminals, scripts, SSH sessions,
//! database tabs and canvas an agent can drive.

use futures::channel::oneshot;
use sawit_core::agents::AgentIdentity;
use sawit_core::app_control::{Command, InstanceAction, Page, Panel, Store, key_bytes};
use sawit_core::control::Preflight;
use sawit_core::db::ConnProfile;
use serde_json::{Value, json};

use super::*;

mod answers;
mod targets;
#[cfg(test)]
mod tests;

use answers::{close_answer, panel_name, script_answer};
use targets::{
    BrowserRoute, agent_project, agent_shell_folder, browser_route, mermaid_op, shell_folder,
    sidebar_tab, split_dir,
};

/// What `app_open_page github` says while signed out: the GitHub work an agent asked for waits on
/// the user's own sign-in, which no tool can do for them.
pub const GITHUB_SIGNED_OUT: &str = "Sign in to GitHub in Sawit first: the GitHub page shows its sign-in card, for the user to sign in. Agents can't sign in for them; app_state's github says when they have.";

/// Terminal lines `terminal_read` returns by default, and at most.
const READ_LINES: usize = 200;
const MAX_READ_LINES: usize = 5_000;

/// A command's answer: now, or once the browser page replies.
pub enum Answer {
    Now(Value),
    Later(oneshot::Receiver<Result<Value, String>>),
}

impl Workspace {
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

    /// Runs a window command. `State`, `Focus`, `Open`, `Reload` and `Keymap` are
    /// handled for the whole app by the caller. `agent_folder` is the folder the agent works in,
    /// when it named no project and a project of this window holds it: its browser and its canvas
    /// are that project's ([`agent_project`]), and so is a new terminal it starts without a folder.
    /// `agent` is the agent session that sent it, whose cursor the canvas and the browser show
    /// where it works (`None` from a `sawit-mcp` that predates identities: an anonymous agent).
    pub fn control(
        &mut self,
        command: Command,
        agent_folder: Option<&Path>,
        agent: Option<&AgentIdentity>,
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
                self.open_page(page, agent_folder, window, cx)?;
                let mut answer = json!({ "tab": self.active });
                // Agents can't sign in: the page shows the user its sign-in card, and says so.
                if page == Page::Github && !self.github.read(cx).is_signed_in() {
                    answer["github"] = self.github.read(cx).describe();
                    answer["note"] = GITHUB_SIGNED_OUT.into();
                }
                done(answer)
            }
            // As a click on the tab: its group unfolds, and its project becomes the active one.
            // The keyboard stays where the user has it, a canvas's tab included.
            Command::ActivateTab { tab } => {
                let ix = self.find_tab(&tab, cx)?;
                self.show_center(cx);
                self.activate_tab(ix, cx);
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
                    let dir = agent_shell_folder(cwd, &self.roots(cx), agent_folder, self.root(cx));
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
                        format!(
                            "Unknown key “{key}”. Use {}.",
                            sawit_core::app_control::KEY_NAMES
                        )
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
            // Unconfirmed, a close that would end what runs there, or the terminal the user types
            // in, is refused: terminal_run never touches the user's shell, and a close shouldn't
            // either without the agent saying it is sure.
            Command::TerminalClose { tab, pane, confirm } => {
                let refusal = self
                    .terminal
                    .read(cx)
                    .close_refusal(tab, pane, window, cx)?;
                if let Some(refusal) = refusal.filter(|_| !confirm) {
                    return Err(refusal);
                }
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
            // The canvas of the agent's project ([`agent_project`]), or of the canvas file it
            // names, where it is: the user's tab, project, layout and keyboard stay as they are,
            // reads included, and a canvas not open yet opens behind the tab on show. It answers
            // at once, with the op's own reply: `{ok: false, error}` when it failed.
            Command::Canvas { op, file } => {
                let board = match file {
                    Some(file) => self.agent_canvas_file(file, cx)?,
                    None => {
                        let root = agent_project(&self.roots(cx), agent_folder, self.root(cx));
                        let board = self.canvas_of(root, super::canvas::Reach::Quietly, cx);
                        board.ok_or("The canvas didn't open.")?
                    }
                };
                // A diagram is rendered off the UI thread, and drawn once it is ready, with the
                // agent's placeholder on the canvas meanwhile.
                if let Some(prepare) = mermaid_op(&op) {
                    return Ok(board.update(cx, |b, cx| b.agent_diagram_op(op, agent, prepare, cx)));
                }
                Ok(board.update(cx, |b, cx| b.agent_op(op, agent, cx)))
            }
            // The browser of the agent's project ([`agent_project`]). One brought forward comes
            // with its project, as on a click on its tab; one used in the background changes
            // nothing on screen.
            Command::Browser { op } => {
                let root = agent_project(&self.roots(cx), agent_folder, self.root(cx));
                let open = self.browser_tab(root.as_deref()).map(|(_, b)| b);
                let shown = open.is_some() && open == self.active_browser();
                let browser = match (browser_route(open.is_some(), shown, op.shows_tab()), open) {
                    (BrowserRoute::Use, Some(browser)) => browser,
                    (BrowserRoute::Closed, _) => {
                        return Err(sawit_core::browser::NOT_OPEN.into());
                    }
                    _ => self.open_browser_of(root, None, window, cx),
                };
                Ok(Answer::Later(
                    browser.update(cx, |b, cx| b.apply_as(op, agent, cx)),
                ))
            }
            // ⌘D or ⇧⌘D for an agent: the new pane shows as terminal_run's terminal does, and the
            // keyboard stays where the user has it. The dock shows first, as for terminal_run: one
            // never on screen can't tell how much room its panes have, and the split asked again
            // once it shows can be measured.
            Command::TerminalSplit {
                tab,
                pane,
                direction,
                command,
                cwd,
            } => {
                let cwd = shell_folder(cwd)?;
                let dir = split_dir(direction);
                self.leave_welcome(TERMINAL_ONLY, cx);
                self.layout.terminal = true;
                cx.notify();
                let (tab, pane) = self.terminal.update(cx, |t, cx| {
                    t.split(
                        tab,
                        pane,
                        dir,
                        cwd,
                        command.as_deref(),
                        Origin::Agent,
                        window,
                        cx,
                    )
                })?;
                let pane = self.terminal.read(cx).pane_index(tab, pane, cx);
                done(json!({ "terminal": tab, "pane": pane }))
            }
            // The keymap is every window's: the caller answers it for the whole app.
            Command::Keymap { .. } => done(json!({})),
            Command::NewProject { location, stack } => {
                done(self.agent_new_project(location, stack, window, cx)?)
            }
        };
        cx.notify();
        result
    }
}
