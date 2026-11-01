//! Ops sent to the app's browser tab, and waiting on its page: the element an op acts on, how long
//! a wait may take, the load a navigation or an action starts, and the dev server to open when no
//! URL is given.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rmcp::model::CallToolResult;
use sawit_core::app_control::Command;
use sawit_core::browser::{BrowserOp, Target, front_project, pick_dev_server};
use serde_json::{Value, json};

use super::{DEFAULT_LOAD_TIMEOUT, MAX_TIMEOUT, POLL, SETTLE};
use crate::app::pretty;
use crate::{Sawit, ok};

/// `back` / `forward` / `reload`, or the page at `url`.
pub(super) fn navigate_op(
    url: Option<String>,
    action: Option<&str>,
) -> Result<Option<BrowserOp>, String> {
    match (url.filter(|u| !u.trim().is_empty()), action.map(str::trim)) {
        (_, Some("back")) => Ok(Some(BrowserOp::Back)),
        (_, Some("forward")) => Ok(Some(BrowserOp::Forward)),
        (_, Some("reload")) => Ok(Some(BrowserOp::Reload)),
        (_, Some(other)) if !other.is_empty() => Err(format!(
            "Unknown action “{other}”: use back, forward or reload."
        )),
        (Some(url), _) => Ok(Some(BrowserOp::Navigate { url })),
        (None, _) => Ok(None),
    }
}

pub(crate) fn target(reference: Option<String>, selector: Option<String>) -> Target {
    let some = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Target {
        reference: some(reference),
        selector: some(selector),
    }
}

pub(crate) fn required(target: Target) -> Result<Target, String> {
    if target.is_empty() {
        return Err("Give a ref (from browser_snapshot) or a CSS selector.".into());
    }
    Ok(target)
}

pub(super) fn timeout(ms: Option<u64>, default: Duration) -> Duration {
    ms.map(Duration::from_millis)
        .unwrap_or(default)
        .min(MAX_TIMEOUT)
}

/// Where a screenshot goes when no path is given.
pub(super) fn screenshot_path(cache: &std::path::Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    cache
        .join("browser")
        .join(format!("screenshot-{stamp}.png"))
}

/// Whether `error`, a page op's, may go away by waiting for the page: the page has no agent to run
/// the op yet while it loads, or WebKit couldn't hand the op to it just now. Nothing else does: no
/// app running, no browser open, a selector that isn't one (MCP-9).
pub(super) fn waits_out(error: &str) -> bool {
    use sawit_core::browser::{NOT_READY, UNREACHABLE};
    error == NOT_READY || error.starts_with(UNREACHABLE)
}

impl Sawit {
    pub(crate) async fn browser(
        &self,
        project: &Option<String>,
        op: BrowserOp,
    ) -> Result<Value, String> {
        self.app_call(project.clone(), Command::Browser { op })
            .await
    }

    /// Polls until the page has loaded (or `limit` passes); the final state,
    /// with `"timed_out": true` when it was still loading.
    pub(super) async fn wait_loaded(
        &self,
        project: &Option<String>,
        limit: Duration,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + limit;
        loop {
            let state = self.browser(project, BrowserOp::State).await?;
            if state["loading"] != true {
                return Ok(state);
            }
            if Instant::now() >= deadline {
                let mut state = state;
                state["timed_out"] = true.into();
                return Ok(state);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// After a click or key: gives a navigation it started time to begin,
    /// then waits for it, so the answer shows where the page ended up.
    pub(super) async fn after_action(
        &self,
        project: &Option<String>,
        mut result: Value,
    ) -> CallToolResult {
        tokio::time::sleep(SETTLE).await;
        if let Ok(state) = self.wait_loaded(project, DEFAULT_LOAD_TIMEOUT).await {
            result["page"] = json!({ "url": state["url"], "title": state["title"], "loading": state["loading"] });
        }
        ok(pretty(&result))
    }

    /// The URL to open when none is given: the dev server running from the
    /// project folder ([`Self::dev_server_folder`]).
    pub(super) async fn dev_server_url(&self, project: &Option<String>) -> Result<String, String> {
        let folder = self
            .dev_server_folder(project, crate::project_dir(None))
            .await?;
        let jobs = sawit_core::probe::listening_jobs()
            .await
            .map_err(|e| e.to_string())?;
        pick_dev_server(&jobs, &folder).ok_or_else(|| {
            "Give a url: no single dev server is running for this project (see list_ports).".into()
        })
    }

    /// The folder whose dev server `browser_navigate` opens when it is given no url (MCP-10):
    /// `project`, as it is on disk, which is how a dev server's folder is listed; else
    /// `agent_folder`, the folder the agent runs in, as the canvas and app tools take it; else,
    /// for an agent run from no project, the project of the front window, which shows the page.
    /// Never the whole Mac: another project's server would open, or none with two running.
    pub(super) async fn dev_server_folder(
        &self,
        project: &Option<String>,
        agent_folder: Option<PathBuf>,
    ) -> Result<PathBuf, String> {
        if let Some(folder) = crate::app::project_path(project.clone()).or(agent_folder) {
            return Ok(folder);
        }
        let state = self.app_call(None, Command::State).await?;
        front_project(&state).ok_or_else(|| {
            "Give a url, or the project whose dev server to open: the front Sawit window has no \
             project."
                .into()
        })
    }
}
