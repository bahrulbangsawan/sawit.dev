//! Tools that drive the running Sawit app over its socket (see
//! `sawit_core::app_control`): windows and tabs, panels, terminals, the Run
//! panel, SSH sessions, database tabs and the canvas.

mod canvas;
mod launch;
mod panels;
mod terminal;
#[cfg(test)]
mod tests;
mod window;

use std::path::PathBuf;
use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::CallToolResult;
use sawit_core::app_control::{self, Command, Request};
use serde_json::Value;

pub use canvas::{
    CanvasCommandArgs, CanvasCommandsArgs, CanvasDeleteArgs, CanvasElementsArgs, CanvasExportArgs,
    CanvasLoadArgs, CanvasMermaidArgs, CanvasTargetArgs, CanvasUpdateArgs,
};
pub use launch::LaunchArgs;
pub use panels::{
    InstanceArgs, NetworkArgs, OpenDiagramArgs, OpenPromptArgs, OpenSqlArgs, OpenTableArgs,
    ScriptArgs, SshConnectArgs,
};
pub use terminal::{
    TerminalCloseArgs, TerminalInputArgs, TerminalReadArgs, TerminalRunArgs, TerminalSplitArgs,
};
pub use window::{LayoutArgs, OpenArgs, PageArgs, PanelArgs, TabArgs, WindowArgs};

use crate::{Sawit, fail, ok};

/// Most commands answer at once; browser ops wait for the page (the app
/// gives up after 30 s).
const REPLY_TIMEOUT: Duration = Duration::from_secs(40);

/// A snake_case name as one of `T`'s variants (`ports` as `Panel::Ports`).
fn variant<T: serde::de::DeserializeOwned>(name: &str, choices: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(name.trim().to_lowercase()))
        .map_err(|_| format!("Unknown “{name}”: use one of {choices}."))
}

const PANELS: &str = "instances, files, git, database, run, ports, network, prompts, ssh, shell_config (Config Files)";
const PAGES: &str = "canvas, browser, mcp, tools, skills, changelog, git_graph, github, instance, keymap, new_project";
const INSTANCE_ACTIONS: &str = "select, start, stop, restart";
const SPLIT_DIRECTIONS: &str = "right, down";

pub(crate) fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// The folder an agent's `project` names, as the app names the projects it opens: `~` expanded, a
/// relative name taken from the agent's folder ([`crate::agent_path`]), then through the links on
/// its way, in the letter case it has on disk and without `..` ([`sawit_core::files::resolve`]).
/// Sent as written, `~/…`, `/tmp/…` (`/private/tmp/…` on macOS) or another case matched no open
/// project, though the same names find the stored canvas with the app closed (MCP-3).
pub(crate) fn project_path(p: Option<String>) -> Option<PathBuf> {
    let p = p.filter(|p| !p.trim().is_empty())?;
    Some(sawit_core::files::resolve(&crate::agent_path(&p)))
}

impl Sawit {
    /// Every tool that drives the app: each area's router, added up in the order its tools were
    /// declared in.
    pub(crate) fn app_router() -> ToolRouter<Sawit> {
        Self::launch_router()
            + Self::window_router()
            + Self::terminal_router()
            + Self::panels_router()
            + Self::canvas_router()
    }

    /// Sends `command` to the running app; the reply's JSON or its error.
    pub(crate) async fn app_call(
        &self,
        project: Option<String>,
        command: Command,
    ) -> Result<Value, String> {
        let paths = self.paths.clone();
        let project = project_path(project);
        // Without a project, the app uses the window that has the agent's folder open.
        let agent_folder = match project {
            Some(_) => None,
            None => crate::project_dir(None),
        };
        let request = Request {
            project,
            agent_folder,
            agent: Some(self.agent.identity()),
            command,
        };
        tokio::task::spawn_blocking(move || app_control::send(&paths, &request, REPLY_TIMEOUT))
            .await
            .map_err(|e| e.to_string())?
    }

    async fn app_tool(&self, project: Option<String>, command: Command) -> CallToolResult {
        match self.app_call(project, command).await {
            Ok(value) => ok(pretty(&value)),
            Err(e) => fail(e),
        }
    }
}

/// An image the app sent as base64; `from` names the sender in the error.
pub(crate) fn base64_decode(text: &str, from: &str) -> Result<Vec<u8>, String> {
    sawit_core::browser::base64_decode(text).ok_or_else(|| format!("{from} sent invalid base64."))
}
