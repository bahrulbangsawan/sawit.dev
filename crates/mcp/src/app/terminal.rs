//! The terminal tools: run a command in Sawit's terminal, split a pane, read a pane's output, type
//! into it, name it and close it.

use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::{self, Command};
use serde_json::Value;

use super::{SPLIT_DIRECTIONS, variant};
use crate::{Sawit, fail, ok, tri};

/// `terminal_run` waits at most this long for output.
pub(super) const MAX_WAIT: Duration = Duration::from_secs(120);

/// How `terminal_run` names where its command ran, for later `terminal_read` and
/// `terminal_input` calls.
fn terminal_name(tab: Option<usize>, pane: Option<usize>) -> String {
    match (tab, pane) {
        (Some(tab), Some(pane)) => format!("Terminal {tab}, pane {pane}"),
        (Some(tab), None) => format!("Terminal {tab}"),
        _ => "Terminal".into(),
    }
}

/// How long `terminal_run` waits before it reads the output: `wait_secs` (default 2), from 0 to
/// [`MAX_WAIT`]. The number is bounded before it becomes a `Duration`, which panics on one too
/// big for it or NaN, and a panic would leave the tool call without an answer.
pub(super) fn run_wait(wait_secs: Option<f64>) -> Duration {
    let secs = wait_secs.filter(|s| !s.is_nan()).unwrap_or(2.0);
    Duration::from_secs_f64(secs.clamp(0.0, MAX_WAIT.as_secs_f64()))
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalRunArgs {
    #[schemars(description = "Command line(s) to type, followed by Enter")]
    pub command: String,
    #[schemars(
        description = "Start a new shell in this folder: an absolute path, or ~/…. A new_tab without cwd starts in the project `project` names, else in your own: the window's project that holds the folder sawit-mcp runs in, else the project in front"
    )]
    pub cwd: Option<String>,
    #[schemars(
        description = "Use a new terminal tab instead of an idle shell (the active one, unless the user is typing in it)"
    )]
    #[serde(default)]
    pub new_tab: bool,
    #[schemars(
        description = "Seconds to wait before returning the terminal's output (0-120, default 2)"
    )]
    pub wait_secs: Option<f64>,
    #[schemars(description = "Output lines to return (default 100)")]
    pub lines: Option<usize>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalReadArgs {
    #[schemars(
        description = "Terminal tab index, as terminal_run, app_run_script and app_ssh_connect return it (or from app_state). Without it: the pane the user has in front, which needn't be where your command ran"
    )]
    pub tab: Option<usize>,
    #[schemars(
        description = "Pane within the tab, as those tools return it (or from app_state); needed when the tab is split"
    )]
    pub pane: Option<usize>,
    #[schemars(description = "Lines of scrollback + screen to return (default 200, max 5000)")]
    pub lines: Option<usize>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalInputArgs {
    #[schemars(
        description = "Terminal tab index, as terminal_run, app_run_script and app_ssh_connect return it (or from app_state)"
    )]
    pub tab: usize,
    #[schemars(
        description = "Pane within the tab, as those tools return it (or from app_state); needed when the tab is split"
    )]
    pub pane: Option<usize>,
    #[schemars(description = "Text to type as is (no Enter added)")]
    pub text: Option<String>,
    #[schemars(description = format!(
        "Key to press after the text: {}; ctrl-c interrupts",
        app_control::KEY_NAMES
    ))]
    pub key: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalCloseArgs {
    #[schemars(
        description = "Terminal tab index, as terminal_run, app_run_script and app_ssh_connect return it (or from app_state)"
    )]
    pub tab: usize,
    #[schemars(description = "Pane within the tab (from app_state); default: the whole tab")]
    pub pane: Option<usize>,
    #[schemars(
        description = "Close it even though closing ends what runs there (a dev server, an SSH session, an agent) or the user is typing in it. Without it such a close is refused, naming what runs"
    )]
    #[serde(default)]
    pub confirm: bool,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalRenameArgs {
    #[schemars(
        description = "Terminal tab index, as terminal_run, app_run_script and app_ssh_connect return it (or from app_state)"
    )]
    pub tab: usize,
    #[schemars(
        description = "Pane within a split tab to name in its header (from app_state); default: the tab itself. A tab of one pane is named itself"
    )]
    pub pane: Option<usize>,
    #[schemars(
        description = "The name to show; an empty one clears it, back to the title the shell gives"
    )]
    pub name: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalSplitArgs {
    #[schemars(
        description = "Terminal tab index, as terminal_run, app_run_script and app_ssh_connect return it (or from app_state)"
    )]
    pub tab: usize,
    #[schemars(
        description = "Pane within the tab to split, as those tools return it (or from app_state); needed when the tab is split"
    )]
    pub pane: Option<usize>,
    #[schemars(
        description = "right (side by side, as ⌘D splits) or down (one above the other, as ⇧⌘D)"
    )]
    pub direction: String,
    #[schemars(description = "Command line(s) to type into the new shell, followed by Enter")]
    pub command: Option<String>,
    #[schemars(
        description = "Folder the new shell starts in: an absolute path, or ~/… (default: the split pane's folder, else its tab's project)"
    )]
    pub cwd: Option<String>,
    #[schemars(
        description = "Seconds to wait before returning the new pane's output (0-120, default 2)"
    )]
    pub wait_secs: Option<f64>,
    #[schemars(description = "Output lines to return (default 100)")]
    pub lines: Option<usize>,
    pub project: Option<String>,
}

impl Sawit {
    /// What `terminal_run` and `terminal_split` answer, once the app said where they started
    /// (`started`: the terminal and the pane in it): that place, then, after waiting `wait_secs`
    /// ([`run_wait`]), that pane's latest `lines` lines (default 100).
    async fn started_output(
        &self,
        project: Option<String>,
        started: &Value,
        wait_secs: Option<f64>,
        lines: Option<usize>,
    ) -> CallToolResult {
        let tab = started["terminal"].as_u64().map(|t| t as usize);
        let pane = started["pane"].as_u64().map(|p| p as usize);
        tokio::time::sleep(run_wait(wait_secs)).await;
        let read = Command::TerminalRead {
            tab,
            pane,
            lines: Some(lines.unwrap_or(100)),
        };
        match self.app_call(project, read).await {
            Ok(v) => ok(format!(
                "{}:\n{}",
                terminal_name(tab, pane),
                v["text"].as_str().unwrap_or_default()
            )),
            Err(e) => fail(e),
        }
    }
}

#[tool_router(router = terminal_router, vis = "pub(super)")]
impl Sawit {
    #[tool(
        description = "Run a command in Sawit's terminal (visible to the user), wait, and return where it ran (\"Terminal 2, pane 0:\") with the terminal's latest output. Pass that tab and pane to terminal_read to check a long-running command later, and to terminal_input (key ctrl-c) to stop it: the terminal in front may be another, one the user types in"
    )]
    pub(super) async fn terminal_run(
        &self,
        Parameters(a): Parameters<TerminalRunArgs>,
    ) -> CallToolResult {
        // A folder the shell can't go to would start it in the app's folder instead (MCP-4).
        let cwd = a.cwd.filter(|cwd| !cwd.trim().is_empty());
        let cwd = match cwd.as_deref().map(crate::existing_dir).transpose() {
            Ok(cwd) => cwd,
            Err(e) => return fail(format!("Nothing was run: {e}")),
        };
        let command = Command::TerminalRun {
            command: a.command,
            cwd,
            new_tab: a.new_tab,
        };
        let started = match self.app_call(a.project.clone(), command).await {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        // Where the command went: the tab and the pane in it, which needn't be the tab's
        // focused pane (the app leaves the focus alone while the user types in a terminal).
        self.started_output(a.project, &started, a.wait_secs, a.lines)
            .await
    }

    #[tool(
        description = "Split a terminal pane in two, as ⌘D (right) or ⇧⌘D (down) does, and start a shell in the new half, optionally running a command there: a dev server or a test watcher beside the user's shell. Name the tab, and the pane in a split tab, as terminal_run or app_state give them. The new pane never takes the keyboard. Each half must keep at least 20 columns by 4 rows: a split that would leave less is refused, saying so, and starts nothing. A terminal that hasn't been on screen yet can't be measured: the first split shows it and is refused, so split it again. Waits, then returns where the new pane is (\"Terminal 2, pane 1:\") with its latest output: pass that tab and pane to terminal_read, terminal_input and terminal_close. The split renumbers the panes after the one split, so read app_state again for any other pane of that tab you use. Needs the app running"
    )]
    pub(super) async fn terminal_split(
        &self,
        Parameters(a): Parameters<TerminalSplitArgs>,
    ) -> CallToolResult {
        let direction = tri!(variant::<app_control::SplitDirection>(
            &a.direction,
            SPLIT_DIRECTIONS
        ));
        // A folder the shell can't go to would start it in the app's folder instead (MCP-4).
        let cwd = a.cwd.filter(|cwd| !cwd.trim().is_empty());
        let cwd = match cwd.as_deref().map(crate::existing_dir).transpose() {
            Ok(cwd) => cwd,
            Err(e) => return fail(format!("Nothing was split: {e}")),
        };
        let command = Command::TerminalSplit {
            tab: a.tab,
            pane: a.pane,
            direction,
            command: a.command,
            cwd,
        };
        let started = match self.app_call(a.project.clone(), command).await {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        self.started_output(a.project, &started, a.wait_secs, a.lines)
            .await
    }

    #[tool(
        description = "Read a terminal pane's latest output (scrollback and screen) as text: name the tab, and the pane in a split tab, as terminal_run returned them; without a tab, the pane the user has in front"
    )]
    pub(super) async fn terminal_read(
        &self,
        Parameters(a): Parameters<TerminalReadArgs>,
    ) -> CallToolResult {
        let command = Command::TerminalRead {
            tab: a.tab,
            pane: a.pane,
            lines: a.lines,
        };
        match self.app_call(a.project, command).await {
            Ok(v) => ok(v["text"].as_str().unwrap_or_default().to_string()),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Type text and/or press a key in a terminal, e.g. answer a prompt, or key ctrl-c to interrupt. Name its tab, and its pane in a split tab, as terminal_run, app_run_script or app_ssh_connect returned them"
    )]
    async fn terminal_input(&self, Parameters(a): Parameters<TerminalInputArgs>) -> CallToolResult {
        let command = Command::TerminalInput {
            tab: Some(a.tab),
            pane: a.pane,
            text: a.text,
            key: a.key,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(
        description = "Name a terminal tab in the tab strip, or one pane of a split tab in its header, as the user's Rename Tab and Rename Pane do: label the tab where your dev server or test watcher runs. An empty name clears it, back to the title the shell gives. Name the tab, and a pane, as terminal_run or app_state give them. Returns the tab, the pane, whether the tab or the pane was named, and the name shown. Needs the app running"
    )]
    pub(super) async fn terminal_rename(
        &self,
        Parameters(a): Parameters<TerminalRenameArgs>,
    ) -> CallToolResult {
        self.app_tool(
            a.project,
            Command::TerminalRename {
                tab: a.tab,
                pane: a.pane,
                name: a.name,
            },
        )
        .await
    }

    #[tool(
        description = "Close a terminal tab, or just one pane of it, ending whatever runs there: a dev server, an SSH session, the user's own agent. DESTRUCTIVE: while a program runs there or the user is typing in it, the close is refused unless confirm is true, and the refusal names what runs; shells waiting at their prompt close without it. Name the tab, and a pane to close only that one, as terminal_run or app_state give them"
    )]
    pub(super) async fn terminal_close(
        &self,
        Parameters(a): Parameters<TerminalCloseArgs>,
    ) -> CallToolResult {
        self.app_tool(
            a.project,
            Command::TerminalClose {
                tab: a.tab,
                pane: a.pane,
                confirm: a.confirm,
            },
        )
        .await
    }
}
