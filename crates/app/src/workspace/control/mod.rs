//! `sawit-mcp` commands for one window (see `sawit_core::app_control`):
//! what it shows, and the panels, tabs, terminals, scripts, SSH sessions,
//! database tabs and canvas an agent can drive.

use futures::channel::oneshot;
use sawit_core::agents::AgentIdentity;
use sawit_core::app_control::{Command, FillPanel, InstanceAction, Page, Panel, Store, key_bytes};
use sawit_core::control::Preflight;
use sawit_core::db::ConnProfile;
use serde_json::{Value, json};

use super::*;

mod answers;
mod tabs;
mod targets;
#[cfg(test)]
mod tests;

use answers::{close_answer, panel_name, script_answer};
use targets::{
    agent_project, agent_shell_folder, mermaid_op, shell_folder, sidebar_tab, split_dir,
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
                self.filled_tab = None;
                done(json!({
                    "sidebar": layout.sidebar,
                    "center": layout.center,
                    "terminal": layout.terminal,
                }))
            }
            Command::MaximizeTerminal => {
                self.toggle_fill(FillPanel::Terminal, Origin::Agent, window, cx);
                done(self.describe_fill())
            }
            Command::Maximize { panel } => {
                self.toggle_fill(panel, Origin::Agent, window, cx);
                done(self.describe_fill())
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
                close_when_done,
            } => {
                let cwd = shell_folder(cwd)?;
                self.leave_welcome(TERMINAL_ONLY, cx);
                self.layout.terminal = true;
                // A new tab without a folder starts in the agent's own project.
                let new_dir =
                    agent_shell_folder(None, &self.roots(cx), agent_folder, self.root(cx));
                let agent_id = agent.map(|a| a.id.as_str());
                let ran = self.terminal.update(cx, |t, cx| {
                    t.run_for_agent(
                        &command,
                        cwd,
                        new_dir,
                        new_tab,
                        close_when_done,
                        agent_id,
                        window,
                        cx,
                    )
                })?;
                cx.notify();
                let pane = self.terminal.read(cx).pane_index(ran.tab, ran.pane, cx);
                done(json!({
                    "terminal": ran.tab,
                    "pane": pane,
                    "run": ran.run,
                    "reused": ran.reused,
                    "closes": ran.closes,
                }))
            }
            Command::TerminalRead {
                tab,
                pane,
                lines,
                run,
            } => {
                let lines = lines.unwrap_or(READ_LINES).clamp(1, MAX_READ_LINES);
                let terminal = self.terminal.read(cx);
                let text = match run {
                    Some(run) => terminal.run_text(run, lines, cx)?,
                    None => terminal.text(tab, pane, lines, cx)?,
                };
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
            Command::TerminalClose {
                tab,
                confirm,
                scope: Some(scope),
                ..
            } => {
                let scope = crate::terminal::CloseScope::parse(&scope).ok_or_else(|| {
                    format!("Unknown scope {scope}: use others, left, right, idle or all.")
                })?;
                done(
                    self.terminal
                        .update(cx, |t, cx| t.close_scope_for_agent(scope, tab, confirm, cx))?,
                )
            }
            Command::TerminalClear { tab, pane } => done(
                self.terminal
                    .update(cx, |t, cx| t.clear_for_agent(tab, pane, cx))?,
            ),
            Command::CloseAllTabs { force, confirm } => {
                done(self.close_all_for_agent(force, confirm, window, cx)?)
            }
            Command::TerminalClose {
                tab, pane, confirm, ..
            } => {
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
            Command::TerminalLock { tab, locked } => {
                if tab >= self.terminal.read(cx).tab_count() {
                    return Err(format!("There is no terminal {tab}."));
                }
                let locked = self
                    .terminal
                    .update(cx, |t, cx| t.set_locked(tab, locked, cx));
                done(json!({ "terminal": tab, "locked": locked }))
            }
            Command::TerminalPin { tab, pinned } => {
                if tab >= self.terminal.read(cx).tab_count() {
                    return Err(format!("There is no terminal {tab}."));
                }
                let (pinned, at) = self
                    .terminal
                    .update(cx, |t, cx| t.set_pinned(tab, pinned, cx));
                done(json!({ "terminal": at, "pinned": pinned }))
            }
            Command::TerminalRename { tab, pane, name } => done(
                self.terminal
                    .update(cx, |t, cx| t.rename(tab, pane, &name, cx))?,
            ),
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
            // The browser of the agent's project ([`agent_project`]), or one of its tabs.
            Command::Browser { op, tab } => {
                let root = agent_project(&self.roots(cx), agent_folder, self.root(cx));
                self.agent_browser_op(root, op, tab, agent, window, cx)
            }
            Command::BrowserTabs {
                action,
                tab,
                url,
                show,
            } => {
                let root = agent_project(&self.roots(cx), agent_folder, self.root(cx));
                self.agent_browser_tabs(root, &action, tab, url, show, window, cx)
                    .map(Answer::Now)
            }
            Command::CanvasTabs {
                action,
                tab,
                name,
                show,
            } => {
                let root = agent_project(&self.roots(cx), agent_folder, self.root(cx));
                self.agent_canvas_tabs(root, &action, tab, name, show, window, cx)
                    .map(Answer::Now)
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
            Command::NewProject {
                location,
                stack,
                open,
                cli_version,
            } => done(self.agent_new_project(location, stack, open, cli_version, window, cx)?),
        };
        cx.notify();
        result
    }
}
