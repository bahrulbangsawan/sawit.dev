//! `launch_app`: starts Sawit through the `sawit` binary beside `sawit-mcp`, or brings it to the
//! front, and waits until it answers.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::{self, Command, Request};

use super::REPLY_TIMEOUT;
use crate::{Sawit, fail, ok};

/// How long `launch_app` waits for the app to listen.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LaunchArgs {
    #[schemars(
        description = "Absolute folder or file (or ~/…) to open once Sawit runs; a file that isn't there opens as a new one"
    )]
    pub path: Option<String>,
}

#[tool_router(router = launch_router, vis = "pub(super)")]
impl Sawit {
    #[tool(
        description = "Start the Sawit app if it isn't running (or bring it forward), optionally opening a folder or file. The app_*, terminal_* and browser_* tools need it running; the canvas_* tools also work on the saved canvas file without it. The keymap_* tools and terminal_split need it running too"
    )]
    async fn launch_app(&self, Parameters(a): Parameters<LaunchArgs>) -> CallToolResult {
        let paths = self.paths.clone();
        let result = tokio::task::spawn_blocking(move || launch(&paths, a.path.as_deref()))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        match result {
            Ok(text) => ok(text),
            Err(e) => fail(e),
        }
    }
}

/// Starts the app through the `sawit` binary next to `sawit-mcp` (it
/// launches Sawit.app or a detached app process), then waits for it.
fn launch(paths: &sawit_core::instances::AppPaths, path: Option<&str>) -> Result<String, String> {
    let target = launch_target(path)?;
    let path = target.as_deref();
    let running = app_control::is_running(paths);
    if running && path.is_none() {
        let request = Request {
            project: None,
            agent_folder: None,
            agent: None,
            command: Command::Focus,
        };
        app_control::send(paths, &request, REPLY_TIMEOUT)?;
        return Ok("Sawit is running; brought it to the front.".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let sawit = exe.with_file_name("sawit");
    let mut cmd = if sawit.is_file() {
        std::process::Command::new(&sawit)
    } else {
        let mut open = std::process::Command::new("/usr/bin/open");
        open.args(["-a", "Sawit"]);
        if path.is_some() {
            open.arg("--args");
        }
        open
    };
    cmd.args(launch_args(path));
    let status = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Unable to start Sawit: {e}"))?;
    if !status.status.success() {
        return Err(format!(
            "Unable to start Sawit: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    let deadline = std::time::Instant::now() + LAUNCH_TIMEOUT;
    while !app_control::is_running(paths) {
        if std::time::Instant::now() > deadline {
            return Err("Sawit started but isn't answering yet. Try app_state in a moment.".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(launched(path))
}

/// What `launch_app` opens for `path`: nothing for none or a blank one, else the path with `~` for
/// the home folder, which must then be absolute, as `app_open`'s must, taken through the links on
/// its way as the CLI and the app name what they open ([`sawit_core::files::resolve`]). The CLI
/// read it as written (MCP-5): `~/proj` was a new file under `<the agent's folder>/~/proj`.
pub(super) fn launch_target(path: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) else {
        return Ok(None);
    };
    let expanded = crate::expand_home(path);
    if !expanded.is_absolute() {
        return Err(format!(
            "“{path}” must be an absolute path (or start with ~/): nothing was opened."
        ));
    }
    Ok(Some(sawit_core::files::resolve(&expanded)))
}

/// The arguments the `sawit` CLI gets to open `path`: `--`, then the path, so that nothing in it
/// is read as an option (`-n`, `--version`) or a subcommand (`list`, a database's name).
pub(super) fn launch_args(path: Option<&Path>) -> Vec<std::ffi::OsString> {
    match path {
        Some(path) => vec!["--".into(), path.as_os_str().to_owned()],
        None => Vec::new(),
    }
}

/// `launch_app`'s answer once Sawit runs, having opened `path`: the path as it opened it, and a
/// path with nothing there is a new file, as `sawit <path>` opens one.
pub(super) fn launched(path: Option<&Path>) -> String {
    match path {
        Some(path) if path.exists() => format!("Sawit is running with {} open.", path.display()),
        Some(path) => format!(
            "Sawit is running with {} open as a new file: nothing was there.",
            path.display()
        ),
        None => "Sawit is running.".into(),
    }
}
