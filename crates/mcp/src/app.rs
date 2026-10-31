//! Tools that drive the running Sawit app over its socket (see
//! `sawit_core::app_control`): windows and tabs, panels, terminals, the Run
//! panel, SSH sessions, database tabs and the canvas.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_board::store::{self, Stamp, Writer};
use sawit_board::{BoardDoc, ops};
use sawit_core::app_control::{self, Command, InstanceAction, Page, Panel, Request};
use serde_json::{Value, json};

use crate::{Sawit, blocking, fail, ok, tri};

/// Most commands answer at once; browser ops wait for the page (the app
/// gives up after 30 s).
const REPLY_TIMEOUT: Duration = Duration::from_secs(40);
/// How long `launch_app` waits for the app to listen.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
/// `terminal_run` waits at most this long for output.
const MAX_WAIT: Duration = Duration::from_secs(120);

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
fn run_wait(wait_secs: Option<f64>) -> Duration {
    let secs = wait_secs.filter(|s| !s.is_nan()).unwrap_or(2.0);
    Duration::from_secs_f64(secs.clamp(0.0, MAX_WAIT.as_secs_f64()))
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WindowArgs {
    #[schemars(
        description = "Project folder (or a path inside it) whose window to use; default the window with your working folder open, else the frontmost"
    )]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LaunchArgs {
    #[schemars(
        description = "Absolute folder or file (or ~/…) to open once Sawit runs; a file that isn't there opens as a new one"
    )]
    pub path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenArgs {
    #[schemars(description = "Absolute path of a file or folder to open")]
    pub path: String,
    #[schemars(description = "1-based line to put the cursor on (files)")]
    pub line: Option<u32>,
    #[schemars(description = "1-based column (files)")]
    pub column: Option<u32>,
    #[schemars(description = "Always open a new window")]
    #[serde(default)]
    pub new_window: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PanelArgs {
    #[schemars(
        description = "instances, files, git, database, run, ports, network, prompts, ssh or shell_config (the Config Files panel)"
    )]
    pub panel: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LayoutArgs {
    #[schemars(description = "Show (true) or hide (false) the left sidebar")]
    pub sidebar: Option<bool>,
    #[schemars(description = "Show or hide the bottom terminal dock")]
    pub terminal: Option<bool>,
    #[schemars(description = "Show or hide the tabs (center); tabs or terminal stay visible")]
    pub center: Option<bool>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    #[schemars(
        description = "canvas, browser, mcp, tools, changelog, git_graph, github, instance or keymap"
    )]
    pub page: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TabArgs {
    #[schemars(description = "Tab index (from app_state) or title")]
    pub tab: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalRunArgs {
    #[schemars(description = "Command line(s) to type, followed by Enter")]
    pub command: String,
    #[schemars(
        description = "Start a new shell in this folder: an absolute path, or ~/… (default: the project)"
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
    #[schemars(
        description = "Key to press after the text: enter, tab, escape, backspace, up, down, left, right, home, end, pageup, pagedown, delete or ctrl-<letter> (ctrl-c interrupts)"
    )]
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
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScriptArgs {
    #[schemars(description = "Script or saved command name as app_run_list shows it")]
    pub name: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SshConnectArgs {
    #[schemars(description = "Saved server id or name, or a ~/.ssh/config alias")]
    pub host: String,
    #[schemars(description = "Open another session even if one is open")]
    #[serde(default)]
    pub new_session: bool,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenTableArgs {
    #[schemars(description = "Saved connection name or Sawit instance name")]
    pub connection: String,
    pub schema: String,
    pub table: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenSqlArgs {
    #[schemars(description = "Saved connection name or Sawit instance name")]
    pub connection: String,
    #[schemars(description = "SQL to put in the editor")]
    pub sql: Option<String>,
    #[schemars(
        description = "Run it right away (default false). It runs with the connection's own privileges, like execute_sql"
    )]
    pub run: Option<bool>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstanceArgs {
    #[schemars(description = "Sawit instance name (see list_instances)")]
    pub name: String,
    #[schemars(description = "select, start, stop or restart")]
    pub action: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NetworkArgs {
    #[schemars(description = "Also ping this host or IP (4 pings)")]
    pub ping: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenDiagramArgs {
    #[schemars(description = "Saved connection name or Sawit instance name")]
    pub connection: String,
    #[schemars(description = "Schema to draw, e.g. public")]
    pub schema: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenPromptArgs {
    #[schemars(description = "Prompt id (see list_prompts)")]
    pub id: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasElementsArgs {
    #[schemars(
        description = "Element skeletons, e.g. {\"type\":\"rectangle\",\"id\":\"a\",\"x\":0,\"y\":0,\"width\":160,\"height\":60,\"label\":{\"text\":\"API\"}}, {\"type\":\"arrow\",\"x\":160,\"y\":30,\"start\":{\"id\":\"a\"},\"end\":{\"id\":\"b\"}}, {\"type\":\"text\",\"x\":0,\"y\":-40,\"text\":\"Title\"}. Types: rectangle, ellipse, diamond, sticky, text, arrow, line, frame. With no colour given, shapes and stickies are sticky yellow (#FFE78A): pass backgroundColor for another, \"transparent\" for no fill. A line through more than two points is drawn as ink, #1E1E1E unless strokeColor names another. Arrows and lines run straight"
    )]
    pub elements: Vec<Value>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasMermaidArgs {
    #[schemars(description = "Mermaid source (flowchart, sequence, class, ...)")]
    pub source: String,
    #[schemars(description = "Where to put it; default right of the existing content")]
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasUpdateArgs {
    #[schemars(
        description = "Changes per element: {\"id\":\"a\", \"x\":10, \"text\":\"New label\", \"strokeColor\":\"#e03131\", \"backgroundColor\":\"#ffc9c9\", ...}"
    )]
    pub elements: Vec<Value>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasDeleteArgs {
    #[schemars(description = "Element ids (from canvas_get)")]
    pub ids: Vec<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasLoadArgs {
    #[schemars(
        description = "A .figcraft file to load, or an .excalidraw file to import (replaces the board). A backup Sawit keeps loads too: canvas.figcraft.bak brings back the board a clear or a load took off"
    )]
    pub path: Option<String>,
    #[schemars(description = "Or the .excalidraw scene JSON itself")]
    pub scene: Option<Value>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasCommandArgs {
    #[schemars(
        description = "Command id, as canvas_commands lists them: board.sticky, node.set, align.left, edit.undo, ..."
    )]
    pub command: String,
    #[schemars(
        description = "The command's parameters as an object, e.g. {\"x\": 0, \"y\": 0, \"text\": \"Idea\"} for board.sticky"
    )]
    pub params: Option<Value>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasCommandsArgs {
    #[schemars(
        description = "Only the commands whose id or label contains this text (any case), such as board. or align, each with the parameters it takes. Without it: every command's id and label"
    )]
    pub filter: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasExportArgs {
    #[schemars(description = "svg (default) or png")]
    pub format: Option<String>,
    #[schemars(
        description = "Absolute path (or ~/…) to write the file to; default the project's .nyawit/exports folder (Sawit's cache folder outside a project)"
    )]
    pub output_path: Option<String>,
    pub project: Option<String>,
}

/// A snake_case name as one of `T`'s variants (`ports` as `Panel::Ports`).
fn variant<T: serde::de::DeserializeOwned>(name: &str, choices: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(name.trim().to_lowercase()))
        .map_err(|_| format!("Unknown “{name}”: use one of {choices}."))
}

const PANELS: &str = "instances, files, git, database, run, ports, network, prompts, ssh, shell_config (Config Files)";
const PAGES: &str = "canvas, browser, mcp, tools, changelog, git_graph, github, instance, keymap";
const INSTANCE_ACTIONS: &str = "select, start, stop, restart";

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

    /// Runs canvas op `op` and answers it. A running app answers every canvas tool, as it always
    /// has. With no app running, the op runs on the stored canvas file itself ([`headless`]),
    /// which is how an agent edits a canvas while Sawit is closed. The two are never both at work,
    /// so the app's autosave and the file never race.
    async fn canvas(&self, project: Option<String>, op: Value) -> Result<Value, String> {
        if !app_control::is_running(&self.paths) {
            let file = self.canvas_board_file(project.as_deref())?;
            let lock = self.paths.root.join(CANVAS_LOCK);
            return blocking(move || headless(&file, &lock, &op)).await?;
        }
        op_reply(self.app_call(project, Command::Canvas { op }).await?)
    }

    /// The file `project`'s canvas is saved in while the app is closed: its
    /// `.nyawit/canvas.figcraft` (the project is the folder the agent runs in, unless it names
    /// one), else `canvas.figcraft` in Sawit's own folder. This is the app's choice of file too.
    ///
    /// A project the agent names must be a folder that is there, as the app answers only for a
    /// project one of its windows has open. A name that is no folder (a project's name rather than
    /// its path, say) would otherwise get a canvas of its own, in a new folder of that name, where
    /// neither the agent nor the user would look.
    fn canvas_board_file(&self, project: Option<&str>) -> Result<PathBuf, String> {
        let named = project.is_some_and(|project| !project.trim().is_empty());
        let dir = crate::project_dir(project);
        if named
            && let Some(dir) = &dir
            && !dir.is_dir()
        {
            return Err(format!(
                "{} is not a folder: name the project by its folder's absolute path.",
                dir.display()
            ));
        }
        Ok(store::board_file(dir.as_deref(), &self.paths.root))
    }

    pub async fn canvas_get_tool(&self, a: WindowArgs) -> CallToolResult {
        let reply = if app_control::is_running(&self.paths) {
            self.canvas(a.project, json!({ "op": "get" })).await
        } else {
            self.canvas_get_stored(a.project).await
        };
        match reply {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    pub async fn canvas_command_tool(&self, a: CanvasCommandArgs) -> CallToolResult {
        let params = a
            .params
            .map(|params| with_files_resolved(&a.command, params));
        let op = json!({ "op": "command", "command": a.command, "params": params });
        match self.canvas(a.project, op).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    /// `canvas_get` with the app closed: the layers of the stored canvas, as the app would show
    /// them. A project that has only its old Excalidraw canvas is read through the importer, and
    /// that file stays as it is.
    ///
    /// The one exception is an old canvas the importer can't read, while no canvas file has been
    /// made from it: the old reader says what it always did. Once the canvas file is there its own
    /// error is the answer, because the old canvas is months behind it.
    async fn canvas_get_stored(&self, project: Option<String>) -> Result<Value, String> {
        let file = self.canvas_board_file(project.as_deref())?;
        match self.canvas(project.clone(), json!({ "op": "get" })).await {
            Err(_) if !file.exists() => self.canvas_file_summary(project.as_deref()),
            reply => reply,
        }
    }

    /// The old canvas as saved on disk, listed as `canvas_get` listed it before the native canvas:
    /// the answer of [`Self::canvas_get_stored`] for an old canvas the importer can't read. It is
    /// the project's `.nyawit/canvas.excalidraw` ([`sawit_core::nyawit::legacy_canvas_file`]),
    /// else the app-wide one.
    fn canvas_file_summary(&self, project: Option<&str>) -> Result<Value, String> {
        let file = match crate::project_dir(project) {
            Some(project) => sawit_core::nyawit::legacy_canvas_file(&project),
            None => self.paths.root.join("canvas.excalidraw"),
        };
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(json!({ "count": 0, "elements": [], "source": "file" }));
            }
            Err(e) => return Err(format!("Unable to read {}: {e}", file.display())),
        };
        let scene: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} isn't valid JSON: {e}", file.display()))?;
        let elements: Vec<Value> = scene["elements"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|e| e["isDeleted"] != true)
                    .map(summarize_element)
                    .collect()
            })
            .unwrap_or_default();
        Ok(
            json!({ "count": elements.len(), "elements": elements, "source": "file (Sawit isn't running)" }),
        )
    }
}

/// The fields `canvas_get` shows for one saved element.
fn summarize_element(e: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for key in [
        "id",
        "type",
        "x",
        "y",
        "width",
        "height",
        "text",
        "strokeColor",
        "backgroundColor",
        "groupIds",
        "frameId",
    ] {
        if let Some(v) = e.get(key).filter(|v| !v.is_null()) {
            out.insert(key.into(), v.clone());
        }
    }
    for binding in ["startBinding", "endBinding"] {
        if let Some(id) = e[binding]["elementId"].as_str() {
            out.insert(binding.into(), id.into());
        }
    }
    Value::Object(out)
}

/// A canvas op's reply as a tool's result. The native canvas answers an op that failed with
/// `{ok: false, error}` (the old canvas's page turned those into errors before they got here), so
/// this is where an agent's failed op becomes a tool error, rather than a success it has to read
/// through. A reply that isn't a failure is as it came.
fn op_reply(reply: Value) -> Result<Value, String> {
    if reply.get("ok") != Some(&Value::Bool(false)) {
        return Ok(reply);
    }
    let error = reply.get("error").and_then(Value::as_str);
    Err(error.unwrap_or("The canvas op failed.").to_string())
}

/// The `commands` op's reply as `canvas_commands` shows it. The engine's list is over 500 commands
/// and 120 KB, most of it the parameters each documents, and a tool's output is cut at 100 KB: the
/// commands at its end would be lost, and those are the board's own. So the whole list is each
/// command's id and label, and the parameters come with the commands that match `filter`, a
/// case-insensitive part of an id or a label.
fn command_list(reply: &Value, filter: Option<&str>) -> String {
    let filter = filter
        .map(|f| f.trim().to_lowercase())
        .filter(|f| !f.is_empty());
    let commands = reply["commands"].as_array().map_or(&[][..], Vec::as_slice);
    let shown: Vec<Value> = commands
        .iter()
        .filter_map(|command| {
            let text = |key: &str| command[key].as_str().unwrap_or_default();
            match &filter {
                None => Some(json!({ "id": text("id"), "label": text("label") })),
                Some(part) => (text("id").to_lowercase().contains(part)
                    || text("label").to_lowercase().contains(part))
                .then(|| command.clone()),
            }
        })
        .collect();
    json!({ "ok": true, "commands": shown }).to_string()
}

/// The stored canvas is opened, changed and saved by one call after another: two at once (an
/// agent's parallel tool calls) would each open the file as it was, and the later save would drop
/// the work of the earlier. This keeps one process's calls apart; the lock in Sawit's folder
/// ([`lock_stored_canvas`]) keeps them from another process's (a second agent's `sawit-mcp`).
static STORED_CANVAS: Mutex<()> = Mutex::new(());

fn take_turn() -> MutexGuard<'static, ()> {
    // A call that panicked has left the file whole (it is written by rename), so the next may go on.
    STORED_CANVAS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The file in Sawit's own folder that a `sawit-mcp` holds locked while one of its calls opens,
/// changes and saves a stored canvas ([`lock_stored_canvas`]).
const CANVAS_LOCK: &str = "mcp-canvas.lock";
/// How long a call waits for another process's call on the stored canvas to end.
const CANVAS_LOCK_WAIT: Duration = Duration::from_secs(60);
/// How many times a call reads the stored canvas and runs its op, when something that doesn't
/// take turns (Sawit as it starts, another program) saved the file while the call ran.
const CANVAS_ATTEMPTS: usize = 3;

/// Takes the lock that keeps the canvas calls of every `sawit-mcp` of one Sawit home apart
/// (MCP-1): a call holds it from the moment it stamps the file until its save is on disk, so no
/// other agent's call reads, saves or checks the file in between. The stamp alone couldn't do
/// that: two calls could each check it before either wrote, and the later save then dropped the
/// earlier one's edit though that call had answered ok.
///
/// The lock is `flock`'s, on the file `lock` ([`CANVAS_LOCK`] in Sawit's folder, which every
/// agent's `sawit-mcp` shares): let go when the returned file is dropped, and by the system when
/// the process ends however it ends, so a crash never leaves the canvas locked. A call waits
/// [`CANVAS_LOCK_WAIT`] at most. A link at the lock's name is not followed.
fn lock_stored_canvas(lock: &Path) -> Result<std::fs::File, String> {
    let failed = |e: &dyn std::fmt::Display| {
        format!(
            "The canvas wasn't touched: its lock {} couldn't be taken: {e}",
            lock.display()
        )
    };
    if let Some(dir) = lock.parent() {
        std::fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    if std::fs::symlink_metadata(lock).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(failed(&"it is a link"));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock)
        .map_err(|e| failed(&e))?;
    let deadline = Instant::now() + CANVAS_LOCK_WAIT;
    let mut pause = Duration::from_millis(1);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "Another agent's canvas call has run for over {} s, so this one stopped \
                     waiting for it and changed nothing. Call it again.",
                    CANVAS_LOCK_WAIT.as_secs()
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(failed(&e)),
        }
    }
}

/// Runs canvas op `op` on the canvas stored in `file` while no app is running, as the app would run
/// it, and saves the canvas if the op changed it. The reply is the op's, and an op that fails is
/// an `Err` that has changed nothing in the file.
///
/// - The canvas is opened as the app opens it ([`store::open`]): a canvas file that isn't there is
///   a new, empty board, and one that is missing beside an old `canvas.excalidraw` is that board,
///   imported. Nothing is written until an op changes the canvas, and the old canvas is never
///   touched. A canvas that can't be opened is an error, and then nothing is written, so a blank
///   board never takes its place.
/// - A change is saved as the app's autosave saves it ([`run_and_save`]): through the store's
///   [`Writer`] (a temporary file renamed into place, a `.bak` kept before a save that empties a
///   canvas, and a `.nyawit/` made with its `.gitignore`), with the file's version history and
///   branches, and never over what the save would lose: images the loader left in the file, or
///   what something else wrote to the file since this call read it.
/// - Calls take turns: this process's by [`take_turn`], every `sawit-mcp`'s by the lock `lock`
///   ([`lock_stored_canvas`]), held from the read to the save. So an answer is true: ok when the
///   edit is on disk, an error when nothing was saved.
fn headless(file: &Path, lock: &Path, op: &Value) -> Result<Value, String> {
    let _turn = take_turn();
    headless_turn(file, lock, op)
}

/// One call's turn at the stored canvas in `file`: what [`headless`] does once this process's
/// other calls are out of the way. It holds the lock `lock`, which keeps other processes' calls
/// out, for the whole turn.
fn headless_turn(file: &Path, lock: &Path, op: &Value) -> Result<Value, String> {
    let _held = lock_stored_canvas(lock)?;
    // The fonts the app's text has: Sawit's own (Inter), then the system's, which no app is
    // scanning here, so they are read the first time text is laid out.
    sawit_board::fonts::register();
    sawit_board::scan_system_fonts_on_demand();
    edit_stored(file, op, || {})
}

/// Reads the canvas stored in `file`, runs `op` on it and saves it ([`run_and_save`]).
///
/// Every `sawit-mcp` takes turns ([`lock_stored_canvas`]), but Sawit as it starts, or another
/// program, may still save the file while a call runs: then the call's save is refused (its edit
/// would drop theirs), and it starts again from what is on disk now, up to [`CANVAS_ATTEMPTS`]
/// times, so the agent's op goes through on top of theirs. When the file changed every time, the
/// call saved nothing, and its error says so in words for an agent. `between` runs after each
/// read, before the op: a test writes the file there.
fn edit_stored(file: &Path, op: &Value, mut between: impl FnMut()) -> Result<Value, String> {
    let mut attempt = 1;
    loop {
        // Stamped before it is read, as the app stamps a canvas file it opens: a save made in
        // between by something else makes this call's save refuse, rather than drop its edit.
        let read = Stamp::of(file);
        let mut board = store::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
        between();
        match run_and_save(&mut board, file, read, op) {
            Err(_) if Stamp::of(file) != read && attempt < CANVAS_ATTEMPTS => attempt += 1,
            Err(_) if Stamp::of(file) != read => return Err(changed_while_running(file)),
            done => return done,
        }
    }
}

/// The error of a call on the canvas stored in `file` that saved nothing, because something else
/// saved the file each time the call read it.
fn changed_while_running(file: &Path) -> String {
    format!(
        "Nothing was saved: something else (Sawit as it opened, or another program) saved {} \
         while this call ran, each of the {CANVAS_ATTEMPTS} times it read it. Call it again; \
         canvas_get shows the canvas as it is now.",
        file.display()
    )
}

/// Runs `op` on `board`, read from `file` when the file had stamp `read` ([`Stamp::of`], `None`
/// when there was no file), and saves it there if the op changed the document itself: a new
/// document, as the engine makes one for every edit, is how the app tells too. Selection and view
/// changes are not in it, and neither is a version the engine shows in its place, so none of them
/// saves anything.
///
/// The save is refused, as the app's autosave refuses it, with the reason as the error and the
/// file as it was:
/// - when the file holds images the loader left out, past what it reads from one file, which the
///   save would delete ([`BoardDoc::refuse_dropping_images`]);
/// - when something else wrote to the file since it was read ([`Writer::guard`]): another
///   `sawit-mcp`, say, whose edit the save would drop though its call answered ok.
///
/// A `replace` of a canvas with layers keeps the file it replaces as a backup first, as a save
/// that empties it does: `<file>.bak`, the backups before it moving along ([`Writer::save`]). The
/// app can undo a replace; here every call opens the canvas afresh, so the backups are all that is
/// left of the board, and one was not enough: an agent that started over twice lost the board it
/// replaced first (CANVAS-CORE-3). A refused save keeps no backup either.
fn run_and_save(
    board: &mut BoardDoc,
    file: &Path,
    read: Option<Stamp>,
    op: &Value,
) -> Result<Value, String> {
    let before = board.document_itself().map_err(|e| e.to_string())?;
    let reply = op_reply(ops::run(board, op))?;
    let after = board.document_itself().map_err(|e| e.to_string())?;
    if Arc::ptr_eq(&before, &after) {
        return Ok(reply);
    }
    board
        .refuse_dropping_images(file)
        .map_err(|e| e.to_string())?;
    let bytes = board.to_bytes().map_err(|e| e.to_string())?;
    let backup = op["op"] == "replace" || store::is_empty(&after);
    let writer = Writer::spawn();
    writer.guard(file.to_path_buf(), read);
    writer.save(file.to_path_buf(), bytes, backup);
    writer.flush();
    match writer.last_error() {
        Some(error) => Err(error),
        None => Ok(reply),
    }
}

/// The text of the scene file at `path`, for `canvas_load`: only a regular file of at most
/// [`sawit_board::excalidraw::MAX_SCENE`] bytes is read, as the canvas reads an `.excalidraw`
/// file ([`sawit_core::files::read_regular`]). A named pipe blocked the read for ever, and a link
/// a cloned repository holds to `/dev/zero` read until the Mac ran out of memory (CANVAS-CORE-6);
/// both are refused at once. It blocks, so it runs off the server's async threads.
fn read_scene(path: &Path) -> std::io::Result<String> {
    let bytes = sawit_core::files::read_regular(path, sawit_board::excalidraw::MAX_SCENE)?;
    String::from_utf8(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })
}

/// Whether `path` names a FigCraft file: a `.figcraft` file, or a backup Sawit keeps of one, such as
/// the `canvas.figcraft.bak` a clear keeps ([`ops::is_figcraft_file`]), which loads as the board it
/// was (MCP-6).
fn is_figcraft(path: &str) -> bool {
    ops::is_figcraft_file(Path::new(path.trim()))
}

/// `path` as the app, whose working folder is not the agent's, can open it: `~` expanded, and a
/// relative path taken from the folder the agent runs in. It is text for a JSON op, so a folder
/// that isn't valid UTF-8 comes out with replacement characters, and the canvas says it can't be read.
fn absolute_path(path: &str) -> String {
    crate::agent_path(path).to_string_lossy().into_owned()
}

/// The engine's commands that write or read a file or a folder named in one of their parameters,
/// with that parameter. Every other `path` an engine command takes is no file: SVG path data
/// (`vector.create`, `board.ink`, `node.create`), a layer (`text.onPath`), a place in a prototype's
/// actions (`proto.action.*`).
const FILE_PARAMS: &[(&str, &str)] = &[
    ("file.export", "path"),
    ("file.exportSvg", "path"),
    ("file.exportFramesPdf", "path"),
    ("file.exportLayers", "dir"),
    ("data.fill", "dir"),
    ("variable.export", "path"),
    ("component.playground", "path"),
    ("codeConnect.export", "path"),
    ("codeConnect.import", "path"),
    ("file.import", "path"),
    ("file.placeImage", "path"),
    ("image.place", "path"),
    ("library.add", "path"),
    ("text.loadFont", "path"),
    ("collab.recoverFile", "path"),
];

/// `params` of engine command `command`, with the file or folder it names ([`FILE_PARAMS`]) made
/// absolute as the agent means it ([`absolute_path`]), as canvas_load makes its own: the app,
/// whose folder is not the agent's, runs the command, and the engine reads no `~` (MCP-8). The
/// steps of a `command.batch` are read the same way. A share link (`library.add` takes one) and
/// every other parameter go as they are.
fn with_files_resolved(command: &str, mut params: Value) -> Value {
    if command == "command.batch" {
        let steps = params.get_mut("commands").and_then(Value::as_array_mut);
        for step in steps.into_iter().flatten() {
            let id = step
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let (Some(id), Some(params)) = (id, step.get_mut("params")) {
                *params = with_files_resolved(&id, params.take());
            }
        }
        return params;
    }
    let names_a_file = FILE_PARAMS.iter().find(|(id, _)| *id == command);
    if let Some((_, key)) = names_a_file
        && let Some(Value::String(path)) = params.get_mut(*key)
        && !path.trim().is_empty()
        && !path.contains("://")
    {
        *path = absolute_path(path);
    }
    params
}

/// Where an export goes when no path is given: the project's
/// `.nyawit/exports/`, else `<cache>/canvas`.
fn export_path(project: Option<&Path>, cache: &Path, ext: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let name = format!("canvas-{stamp}.{ext}");
    match project {
        Some(project) => {
            let dir = sawit_core::nyawit::exports_dir(project);
            let _ = sawit_core::nyawit::ensure(project, &dir);
            dir.join(name)
        }
        None => cache.join("canvas").join(name),
    }
}

/// An image the app sent as base64; `from` names the sender in the error.
pub(crate) fn base64_decode(text: &str, from: &str) -> Result<Vec<u8>, String> {
    sawit_core::browser::base64_decode(text).ok_or_else(|| format!("{from} sent invalid base64."))
}

#[tool_router(router = app_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Start the Sawit app if it isn't running (or bring it forward), optionally opening a folder or file. The app_*, terminal_* and browser_* tools need it running; the canvas_* tools also work on the saved canvas file without it"
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

    #[tool(
        description = "What the Sawit app shows: each window's project, sidebar panel, layout, open tabs (index, title, kind, path, unsaved) and terminal tabs (index, title, running job, script, ssh) and each tab's split panes"
    )]
    async fn app_state(&self) -> CallToolResult {
        self.app_tool(None, Command::State).await
    }

    #[tool(
        description = "Open a folder as a project or a file (at a line/column) in Sawit, like `sawit <path>`"
    )]
    async fn app_open(&self, Parameters(a): Parameters<OpenArgs>) -> CallToolResult {
        let command = Command::Open {
            path: PathBuf::from(a.path),
            line: a.line,
            column: a.column,
            new_window: a.new_window,
        };
        self.app_tool(None, command).await
    }

    #[tool(description = "Bring a Sawit window to the front")]
    async fn app_focus(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::Focus).await
    }

    #[tool(description = "Show a sidebar panel in Sawit")]
    async fn app_show_panel(&self, Parameters(a): Parameters<PanelArgs>) -> CallToolResult {
        match variant::<Panel>(&a.panel, PANELS) {
            Ok(panel) => self.app_tool(a.project, Command::ShowPanel { panel }).await,
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Show or hide the sidebar, the tabs and the terminal dock")]
    async fn app_layout(&self, Parameters(a): Parameters<LayoutArgs>) -> CallToolResult {
        let command = Command::Layout {
            sidebar: a.sidebar,
            terminal: a.terminal,
            center: a.center,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(description = "Toggle filling the window with the terminal (and back)")]
    async fn app_maximize_terminal(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::MaximizeTerminal).await
    }

    #[tool(
        description = "Open a page as a tab: canvas (whiteboard and design canvas), browser (web browser agents can drive), mcp (install page), tools (dev tools installer), changelog, git_graph, github, instance (selected PostgreSQL instance), keymap (keymap editor)"
    )]
    async fn app_open_page(&self, Parameters(a): Parameters<PageArgs>) -> CallToolResult {
        match variant::<Page>(&a.page, PAGES) {
            Ok(page) => self.app_tool(a.project, Command::OpenPage { page }).await,
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Switch to a center tab by index or title")]
    async fn app_activate_tab(&self, Parameters(a): Parameters<TabArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::ActivateTab { tab: a.tab })
            .await
    }

    #[tool(
        description = "Close a center tab by index or title (a file with unsaved changes asks the user first)"
    )]
    async fn app_close_tab(&self, Parameters(a): Parameters<TabArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::CloseTab { tab: a.tab })
            .await
    }

    #[tool(
        description = "Run a command in Sawit's terminal (visible to the user), wait, and return where it ran (\"Terminal 2, pane 0:\") with the terminal's latest output. Pass that tab and pane to terminal_read to check a long-running command later, and to terminal_input (key ctrl-c) to stop it: the terminal in front may be another, one the user types in"
    )]
    async fn terminal_run(&self, Parameters(a): Parameters<TerminalRunArgs>) -> CallToolResult {
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
        let tab = started["terminal"].as_u64().map(|t| t as usize);
        let pane = started["pane"].as_u64().map(|p| p as usize);
        tokio::time::sleep(run_wait(a.wait_secs)).await;
        let read = Command::TerminalRead {
            tab,
            pane,
            lines: Some(a.lines.unwrap_or(100)),
        };
        match self.app_call(a.project, read).await {
            Ok(v) => ok(format!(
                "{}:\n{}",
                terminal_name(tab, pane),
                v["text"].as_str().unwrap_or_default()
            )),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Read a terminal tab's latest output (scrollback and screen) as text")]
    async fn terminal_read(&self, Parameters(a): Parameters<TerminalReadArgs>) -> CallToolResult {
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

    #[tool(description = "Close a terminal tab, or just one pane of it")]
    async fn terminal_close(&self, Parameters(a): Parameters<TerminalCloseArgs>) -> CallToolResult {
        self.app_tool(
            a.project,
            Command::TerminalClose {
                tab: a.tab,
                pane: a.pane,
            },
        )
        .await
    }

    #[tool(
        description = "The Run panel's scripts (package.json, Make, cargo, deno, composer, workspaces) and saved commands for the window's project, with their exact names and whether each is running"
    )]
    async fn app_run_list(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::RunList).await
    }

    #[tool(
        description = "Start a script or saved command from the Run panel in its own terminal tab. Returns the terminal (tab) and pane it runs in, or each one's for a saved command that runs several: pass them to terminal_read for its output and to terminal_input to answer or stop it"
    )]
    async fn app_run_script(&self, Parameters(a): Parameters<ScriptArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::RunScript { name: a.name })
            .await
    }

    #[tool(description = "Stop (Ctrl-C) a running script or saved command from the Run panel")]
    async fn app_stop_script(&self, Parameters(a): Parameters<ScriptArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::StopScript { name: a.name })
            .await
    }

    #[tool(
        description = "Open an interactive SSH session to a saved server or ~/.ssh/config host in Sawit's terminal; returns its terminal and pane, to drive it with terminal_input / terminal_read"
    )]
    async fn app_ssh_connect(&self, Parameters(a): Parameters<SshConnectArgs>) -> CallToolResult {
        let command = Command::SshConnect {
            host: a.host,
            new_session: a.new_session,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(description = "Open a table in Sawit's data grid")]
    async fn app_open_table(&self, Parameters(a): Parameters<OpenTableArgs>) -> CallToolResult {
        let command = Command::OpenTable {
            connection: a.connection,
            schema: a.schema,
            table: a.table,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(
        description = "Open a SQL editor tab for a connection, optionally prefilled with SQL; run=true also runs it in the app, as ⌘↩ does (the results show in the tab; use query or execute_sql to get rows back as text)"
    )]
    async fn app_open_sql(&self, Parameters(a): Parameters<OpenSqlArgs>) -> CallToolResult {
        let command = Command::OpenSql {
            connection: a.connection,
            sql: a.sql,
            run: a.run.unwrap_or(false),
        };
        self.app_tool(a.project, command).await
    }

    #[tool(
        description = "Select a PostgreSQL instance in the app's Instances tab and start, stop or restart it there, as its buttons do (needs the app; start_instance/stop_instance work without it). A start whose port is taken is held and the Instance tab explains the conflict. Returns at once; read the outcome with list_instances or instance_logs"
    )]
    async fn app_instance(&self, Parameters(a): Parameters<InstanceArgs>) -> CallToolResult {
        match variant::<InstanceAction>(&a.action, INSTANCE_ACTIONS) {
            Ok(action) => {
                let command = Command::Instance {
                    name: a.name,
                    action,
                };
                self.app_tool(a.project, command).await
            }
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Run the app's network diagnosis (connection, router, DNS, internet, Wi-Fi sign-in) on its Network tab, optionally pinging a host too. Needs the app; diagnose_network and ping return the same results as text without it"
    )]
    async fn app_network_diagnose(&self, Parameters(a): Parameters<NetworkArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::NetworkDiagnose { ping: a.ping })
            .await
    }

    #[tool(
        description = "Open a schema's diagram (tables and foreign keys, drawn by Pele) as a tab"
    )]
    async fn app_open_diagram(&self, Parameters(a): Parameters<OpenDiagramArgs>) -> CallToolResult {
        let command = Command::OpenDiagram {
            connection: a.connection,
            schema: a.schema,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(description = "Open a saved prompt in its tab")]
    async fn app_open_prompt(&self, Parameters(a): Parameters<OpenPromptArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::OpenPrompt { id: a.id })
            .await
    }

    #[tool(
        description = "List the elements on Sawit's canvas: id, type, position, size, text, color and what a connector joins. Works from the saved file when the app isn't running"
    )]
    async fn canvas_get(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        self.canvas_get_tool(a).await
    }

    #[tool(
        description = "Draw on Sawit's canvas: add shapes, stickies, labels, text and arrows (arrows bind to shapes by id). Shapes and stickies with no backgroundColor come in sticky yellow. Opens the canvas tab when the app is running"
    )]
    async fn canvas_add_elements(
        &self,
        Parameters(a): Parameters<CanvasElementsArgs>,
    ) -> CallToolResult {
        match self
            .canvas(a.project, json!({ "op": "add", "elements": a.elements }))
            .await
        {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Convert a Mermaid diagram into editable shapes on Sawit's canvas: boxes, arrows bound to them, labels. Charts (pie, gantt, xychart, quadrant, journey, sankey, architecture) are placed as one image. (For an SVG image of a Mermaid diagram use render_mermaid)"
    )]
    async fn canvas_add_mermaid(
        &self,
        Parameters(a): Parameters<CanvasMermaidArgs>,
    ) -> CallToolResult {
        let op = json!({ "op": "mermaid", "source": a.source, "x": a.x, "y": a.y });
        match self.canvas(a.project, op).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Change canvas elements by id: position, size, colors, text")]
    async fn canvas_update(&self, Parameters(a): Parameters<CanvasUpdateArgs>) -> CallToolResult {
        match self
            .canvas(a.project, json!({ "op": "update", "elements": a.elements }))
            .await
        {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Delete canvas elements by id")]
    async fn canvas_delete(&self, Parameters(a): Parameters<CanvasDeleteArgs>) -> CallToolResult {
        match self
            .canvas(a.project, json!({ "op": "delete", "ids": a.ids }))
            .await
        {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Clear the whole canvas. The board it held is kept beside the canvas file as canvas.figcraft.bak, the 4 kept before it moving to .bak.1 to .bak.4 (the oldest goes); in the app the user can also undo it"
    )]
    async fn canvas_clear(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        match self.canvas(a.project, json!({ "op": "clear" })).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Replace the canvas with a .figcraft file, or import an .excalidraw file or scene JSON in its place. The board it replaces is kept as canvas.figcraft.bak, as canvas_clear keeps one"
    )]
    async fn canvas_load(&self, Parameters(a): Parameters<CanvasLoadArgs>) -> CallToolResult {
        let op = match (a.scene, a.path) {
            (Some(scene), _) => json!({ "op": "replace", "scene": scene }),
            // A .figcraft file is loaded by the canvas itself, from where it is, which refuses a
            // file it can't read whole, a pipe or a device included.
            (None, Some(path)) if is_figcraft(&path) => {
                json!({ "op": "replace", "path": absolute_path(&path) })
            }
            // Read here, as an agent means the path (MCP-7): trimmed, `~` for the home folder,
            // and from the agent's folder.
            (None, Some(path)) => {
                let file = crate::agent_path(&path);
                let text = match blocking({
                    let file = file.clone();
                    move || read_scene(&file)
                })
                .await
                {
                    Ok(Ok(text)) => text,
                    Ok(Err(e)) => return fail(format!("Unable to read {}: {e}", file.display())),
                    Err(e) => return fail(e),
                };
                match serde_json::from_str::<Value>(&text) {
                    Ok(scene) => json!({ "op": "replace", "scene": scene }),
                    Err(e) => return fail(format!("{} isn't valid JSON: {e}", file.display())),
                }
            }
            (None, None) => return fail("Give a path or a scene."),
        };
        match self.canvas(a.project, op).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Export the canvas as an SVG or PNG file")]
    async fn canvas_export(&self, Parameters(a): Parameters<CanvasExportArgs>) -> CallToolResult {
        let format = a.format.as_deref().map(str::trim).unwrap_or_default();
        let png = match format.to_lowercase().as_str() {
            "svg" | "" => false,
            "png" => true,
            _ => return fail(format!("Unknown format “{format}”: use svg or png.")),
        };
        // Read as every tool that writes a file reads its path (MCP-11): absolute, and never a
        // canvas file, since nothing else holds the board when the app is closed.
        let named = tri!(crate::output_path(
            "canvas_export",
            a.output_path.as_deref()
        ));
        let op = json!({ "op": if png { "export_png" } else { "export_svg" } });
        let project = crate::project_dir(a.project.as_deref());
        let value = match self.canvas(a.project, op).await {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        let bytes = if png {
            match base64_decode(
                value["png_base64"].as_str().unwrap_or_default(),
                "The canvas",
            ) {
                Ok(b) => b,
                Err(e) => return fail(e),
            }
        } else {
            value["svg"]
                .as_str()
                .unwrap_or_default()
                .as_bytes()
                .to_vec()
        };
        let path = named.unwrap_or_else(|| {
            export_path(
                project.as_deref(),
                &self.paths.cache_dir(),
                if png { "png" } else { "svg" },
            )
        });
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return fail(format!("Unable to create {}: {e}", dir.display()));
        }
        match std::fs::write(&path, &bytes) {
            Ok(()) => ok(format!(
                "Canvas exported to {} ({} bytes)",
                path.display(),
                bytes.len()
            )),
            Err(e) => fail(format!("Unable to write {}: {e}", path.display())),
        }
    }

    #[tool(
        description = "Run one command of the canvas engine by its id, with its parameters as JSON: board.sticky {x, y, text}, node.set, align.left, edit.undo, view.zoomFit, ... The reply is the command's result, and a command the engine refuses is an error. canvas_commands lists the ids, and with a filter what each takes. With the app closed each call starts from the saved file, so nothing carries over between calls (no undo, no selection, no view): pass ids, or run the steps in one command.batch"
    )]
    async fn canvas_command(&self, Parameters(a): Parameters<CanvasCommandArgs>) -> CallToolResult {
        self.canvas_command_tool(a).await
    }

    #[tool(
        description = "List the commands canvas_command runs: the id and label of each. The parameters each takes make the full list too long to return at once, so pass `filter` (board., align, sticky, ...) to get the matching commands with theirs"
    )]
    async fn canvas_commands(
        &self,
        Parameters(a): Parameters<CanvasCommandsArgs>,
    ) -> CallToolResult {
        match self.canvas(a.project, json!({ "op": "commands" })).await {
            Ok(reply) => ok(command_list(&reply, a.filter.as_deref())),
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
fn launch_target(path: Option<&str>) -> Result<Option<PathBuf>, String> {
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
fn launch_args(path: Option<&Path>) -> Vec<std::ffi::OsString> {
    match path {
        Some(path) => vec!["--".into(), path.as_os_str().to_owned()],
        None => Vec::new(),
    }
}

/// `launch_app`'s answer once Sawit runs, having opened `path`: the path as it opened it, and a
/// path with nothing there is a new file, as `sawit <path>` opens one.
fn launched(path: Option<&Path>) -> String {
    match path {
        Some(path) if path.exists() => format!("Sawit is running with {} open.", path.display()),
        Some(path) => format!(
            "Sawit is running with {} open as a new file: nothing was there.",
            path.display()
        ),
        None => "Sawit is running.".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc::Receiver;

    use sawit_core::instances::AppPaths;

    use super::*;

    /// The Excalidraw board of `sawit-board`'s importer tests: 12 live elements, which import as
    /// 9 layers.
    const OLD_BOARD: &str = include_str!("../../board/tests/fixtures/excalidraw/basic.excalidraw");

    #[test]
    fn panel_and_page_names() {
        assert_eq!(
            variant::<Panel>("Shell_Config", PANELS),
            Ok(Panel::ShellConfig)
        );
        assert_eq!(variant::<Page>("git_graph", PAGES), Ok(Page::GitGraph));
        assert_eq!(variant::<Page>("keymap", PAGES), Ok(Page::Keymap));
        assert_eq!(
            variant::<InstanceAction>("Restart", INSTANCE_ACTIONS),
            Ok(InstanceAction::Restart)
        );
        assert!(
            variant::<InstanceAction>("burn", INSTANCE_ACTIONS)
                .unwrap_err()
                .contains("restart")
        );
        assert!(
            variant::<Panel>("nope", PANELS)
                .unwrap_err()
                .contains("ports")
        );
    }

    /// `terminal_run` waits as asked, within 0 to 120 s, whatever number the agent sends: a
    /// huge, infinite or NaN `wait_secs` must not panic the tool call, which would never answer.
    #[test]
    fn terminal_run_waits_within_bounds() {
        assert_eq!(run_wait(None), Duration::from_secs(2));
        assert_eq!(run_wait(Some(0.5)), Duration::from_millis(500));
        assert_eq!(run_wait(Some(-3.0)), Duration::ZERO);
        assert_eq!(run_wait(Some(1e20)), MAX_WAIT);
        assert_eq!(run_wait(Some(f64::INFINITY)), MAX_WAIT);
        assert_eq!(run_wait(Some(f64::NEG_INFINITY)), Duration::ZERO);
        assert_eq!(run_wait(Some(f64::NAN)), Duration::from_secs(2));
    }

    /// Reading or typing into a split tab names the pane: its focused pane may be the one the user
    /// types in, while the agent's command runs in the other. Closing without one closes the tab.
    #[test]
    fn only_closing_defaults_to_the_whole_tab() {
        let pane = |schema: schemars::Schema| {
            schema.as_value()["properties"]["pane"]["description"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        for read_or_input in [
            pane(schemars::schema_for!(TerminalReadArgs)),
            pane(schemars::schema_for!(TerminalInputArgs)),
        ] {
            assert!(
                read_or_input.contains("needed when the tab is split"),
                "{read_or_input}"
            );
            assert!(!read_or_input.contains("whole tab"), "{read_or_input}");
        }
        let close = pane(schemars::schema_for!(TerminalCloseArgs));
        assert!(close.contains("the whole tab"), "{close}");
        assert!(!close.contains("focused pane"), "{close}");
    }

    /// Keys go only to a terminal the agent names: `terminal_input` needs the tab, as
    /// `terminal_close` does. Sent to "the active terminal", a ctrl-c meant for the agent's dev
    /// server would interrupt the program the user types in, Claude Code included. Reading still
    /// defaults to the terminal the user has in front.
    #[test]
    fn terminal_input_needs_the_tab() {
        let required = |schema: schemars::Schema| {
            let required = schema.as_value()["required"].as_array().cloned();
            required.unwrap_or_default().contains(&json!("tab"))
        };
        assert!(required(schemars::schema_for!(TerminalInputArgs)));
        assert!(required(schemars::schema_for!(TerminalCloseArgs)));
        assert!(!required(schemars::schema_for!(TerminalReadArgs)));
        let untargeted = serde_json::from_value::<TerminalInputArgs>(json!({ "key": "ctrl-c" }));
        assert!(untargeted.is_err(), "keys went out with no terminal named");
        let tab = |schema: schemars::Schema| {
            schema.as_value()["properties"]["tab"]["description"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        let read = tab(schemars::schema_for!(TerminalReadArgs));
        assert!(read.contains("the user has in front"), "{read}");
        assert!(read.contains("terminal_run"), "{read}");
    }

    /// MCP-3: the project an agent names goes to the app as the folder it is on disk: trimmed,
    /// `~` expanded, a relative name taken from the agent's folder, and through the links on its
    /// way, in the case the folder has and without `..`, as the app names the projects it opens.
    /// It went as written, so `~/…`, `/tmp/…` or another case matched no open project, though the
    /// same names work with the app closed.
    #[test]
    fn a_project_is_sent_as_the_folder_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("shop");
        std::fs::create_dir_all(real.join("web")).unwrap();
        let real = real.canonicalize().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        for named in [
            format!(" {} ", link.display()),
            dir.path().join("SHOP").display().to_string(),
            real.join("web").join("..").display().to_string(),
        ] {
            assert_eq!(
                project_path(Some(named.clone())),
                Some(real.clone()),
                "{named}"
            );
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = Path::new(&home).canonicalize().unwrap();
            assert_eq!(project_path(Some("~".into())), Some(home.clone()));
            assert_eq!(project_path(Some("~/".into())), Some(home));
        }
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(project_path(Some("src".into())), Some(here.join("src")));
        assert_eq!(project_path(Some("  ".into())), None);
        assert_eq!(project_path(None), None);
    }

    /// MCP-5: `launch_app` opens the folder or file its `path` names, and says what it opened.
    /// The `sawit` CLI took the path as written: `~/proj` opened a new file under
    /// `<the agent's folder>/~/proj` while the reply said "~/proj" was open, and `--version`
    /// was read as an option. `~` is the home folder now, the path must be absolute (as for
    /// `app_open`), and it comes after `--`, so nothing in it is an option or a subcommand.
    #[test]
    fn launch_app_opens_the_path_it_names_and_says_what_it_opened() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("shop");
        std::fs::create_dir_all(&real).unwrap();
        let real = real.canonicalize().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let target = launch_target(Some(&format!(" {} ", link.display()))).unwrap();
        assert_eq!(target.as_deref(), Some(real.as_path()));
        assert_eq!(
            launch_args(target.as_deref()),
            vec![std::ffi::OsString::from("--"), real.clone().into()]
        );
        assert_eq!(
            launched(target.as_deref()),
            format!("Sawit is running with {} open.", real.display())
        );
        if let Some(home) = std::env::var_os("HOME") {
            let home = Path::new(&home).canonicalize().unwrap();
            let target = launch_target(Some("~/no such folder")).unwrap();
            assert_eq!(target, Some(home.join("no such folder")));
            // Nothing is there: the app opens it as a new file, and the answer says so.
            let said = launched(target.as_deref());
            assert!(said.contains("new file"), "{said}");
        }
        for not_absolute in ["--version", "list", "proj", "~user/x"] {
            let refused = launch_target(Some(not_absolute)).unwrap_err();
            assert!(refused.contains("absolute"), "{not_absolute}: {refused}");
        }
        assert_eq!(launch_target(Some("  ")), Ok(None));
        assert_eq!(launch_target(None), Ok(None));
        assert!(launch_args(None).is_empty());
    }

    #[test]
    fn decodes_base64() {
        assert_eq!(base64_decode("aGVsbG8=", "The canvas").unwrap(), b"hello");
        assert_eq!(
            base64_decode("iVBO\nRw==", "The canvas").unwrap(),
            [0x89, b'P', b'N', b'G']
        );
        assert_eq!(
            base64_decode("@@", "The browser"),
            Err("The browser sent invalid base64.".into())
        );
    }

    #[test]
    fn summarizes_saved_elements() {
        let e = json!({
            "id": "a1", "type": "arrow", "x": 1, "y": 2, "width": 3, "height": 4,
            "startBinding": {"elementId": "box"}, "endBinding": null, "seed": 99
        });
        assert_eq!(
            summarize_element(&e),
            json!({"id": "a1", "type": "arrow", "x": 1, "y": 2, "width": 3, "height": 4, "startBinding": "box"})
        );
    }

    // The canvas tools, with the app and without it. Each test gives its server a Sawit home of
    // its own, so that no app answers on its socket but the one a test stands in, and names its
    // project folder, so that nothing is written under the repository.

    /// A server whose home is in `dir`.
    fn native(dir: &Path) -> Sawit {
        Sawit::new(AppPaths {
            root: dir.join("home"),
        })
    }

    fn project_arg(project: &Path) -> Option<String> {
        Some(project.display().to_string())
    }

    /// A project folder in `dir`, with nothing in it yet: a project the agent works in for the
    /// first time.
    fn project_in(dir: &Path) -> PathBuf {
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        project
    }

    /// `<project>/.nyawit/canvas.figcraft`.
    fn board_file(project: &Path) -> PathBuf {
        project.join(".nyawit").join("canvas.figcraft")
    }

    /// A canvas file at `path` with `stickies` stickies, written as the app writes one.
    fn write_board(path: &Path, stickies: usize) {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        for n in 0..stickies {
            let sticky = json!({"x": n as f64 * 300.0, "y": 0, "text": format!("note {n}")});
            board.execute("board.sticky", &sticky).unwrap();
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, board.to_bytes().unwrap()).unwrap();
    }

    /// What `get` lists on the canvas file at `path`, opened as the app opens it.
    fn layers_in(path: &Path) -> Vec<Value> {
        let mut board = store::open(path).unwrap();
        let reply = ops::run(&mut board, &json!({"op": "get"}));
        reply["elements"].as_array().unwrap().clone()
    }

    /// The text of a tool's result.
    fn said(result: &CallToolResult) -> String {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    }

    /// The JSON a tool that succeeded answered.
    fn answer(result: &CallToolResult) -> Value {
        assert_ne!(result.is_error, Some(true), "{}", said(result));
        serde_json::from_str(&said(result)).unwrap()
    }

    /// A stand-in for the running app: it listens where the app does, and answers each canvas
    /// request with the next of `answers` (`{ok: true}` once they are used up). The ops it was
    /// asked to run come out of the receiver.
    fn fake_app(paths: &AppPaths, answers: Vec<Value>) -> Receiver<Value> {
        std::fs::create_dir_all(&paths.root).unwrap();
        let listener = UnixListener::bind(paths.cli_socket()).unwrap();
        let (asked, ops) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut answers = answers.into_iter();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut bytes = Vec::new();
                // `is_running` only connects, and sends nothing.
                if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                    continue;
                }
                let Some(Ok(Request {
                    command: Command::Canvas { op },
                    ..
                })) = app_control::decode(&bytes)
                else {
                    break;
                };
                let _ = asked.send(op);
                let answer = answers.next().unwrap_or_else(|| json!({"ok": true}));
                let line = format!("{}\n", app_control::reply_ok(&answer));
                let _ = stream.write_all(line.as_bytes());
            }
        });
        ops
    }

    /// A stand-in for the running app that answers any command, each with the next of `answers`.
    /// The requests come out of the receiver.
    fn fake_app_answering(paths: &AppPaths, answers: Vec<Value>) -> Receiver<Request> {
        std::fs::create_dir_all(&paths.root).unwrap();
        let listener = UnixListener::bind(paths.cli_socket()).unwrap();
        let (asked, requests) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut answers = answers.into_iter();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut bytes = Vec::new();
                if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                    continue;
                }
                let Some(Ok(request)) = app_control::decode(&bytes) else {
                    break;
                };
                let _ = asked.send(request);
                let Some(answer) = answers.next() else { break };
                let line = format!("{}\n", app_control::reply_ok(&answer));
                let _ = stream.write_all(line.as_bytes());
            }
        });
        requests
    }

    /// `terminal_run` reads back the pane the app says the command went to: it needn't be the
    /// tab's focused pane, which the app leaves alone while the user types in a terminal.
    #[tokio::test]
    async fn terminal_run_reads_the_pane_it_ran_in() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let asked = fake_app_answering(
            &server.paths,
            vec![
                json!({"terminal": 2, "pane": 1}),
                json!({"text": "$ make test\nok"}),
            ],
        );
        let ran = server
            .terminal_run(Parameters(TerminalRunArgs {
                command: "make test".into(),
                cwd: None,
                new_tab: false,
                wait_secs: Some(0.0),
                lines: None,
                project: None,
            }))
            .await;
        assert_eq!(said(&ran), "Terminal 2, pane 1:\n$ make test\nok");
        let request = |asked: &Receiver<Request>| {
            asked
                .recv_timeout(Duration::from_secs(5))
                .expect("the app was asked")
                .command
        };
        assert!(matches!(request(&asked), Command::TerminalRun { .. }));
        assert_eq!(
            request(&asked),
            Command::TerminalRead {
                tab: Some(2),
                pane: Some(1),
                lines: Some(100),
            }
        );
    }

    /// MCP-4: `terminal_run` starts its shell in the folder it names, or not at all. The PTY
    /// ignores a `chdir` that fails, so a folder with `~`, a relative one or one that isn't there
    /// started the shell in the app's own folder (`/` for Sawit.app), and the command ran there
    /// though the tool answered ok. `~` is the home folder; a relative or missing folder is an
    /// error, and the app isn't asked.
    #[tokio::test]
    async fn terminal_run_starts_where_it_says_or_not_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let asked = fake_app_answering(
            &server.paths,
            vec![
                json!({"terminal": 1, "pane": 0}),
                json!({"text": ""}),
                json!({"terminal": 2, "pane": 0}),
                json!({"text": ""}),
            ],
        );
        let run = |cwd: &str| TerminalRunArgs {
            command: "rm -rf build".into(),
            cwd: Some(cwd.into()),
            new_tab: false,
            wait_secs: Some(0.0),
            lines: None,
            project: None,
        };
        let cwd_asked = |asked: &Receiver<Request>| {
            let request = asked.recv_timeout(Duration::from_secs(5)).expect("asked");
            let Command::TerminalRun { cwd, .. } = request.command else {
                panic!("{:?}", request.command)
            };
            asked.recv_timeout(Duration::from_secs(5)).expect("read");
            cwd
        };

        // A folder that is there, written with spaces around it, and the home folder as `~`.
        let folder = dir.path().join("web");
        std::fs::create_dir_all(&folder).unwrap();
        let ran = server
            .terminal_run(Parameters(run(&format!(" {} ", folder.display()))))
            .await;
        assert_ne!(ran.is_error, Some(true), "{}", said(&ran));
        assert_eq!(cwd_asked(&asked), Some(folder));
        if let Some(home) = std::env::var_os("HOME") {
            let ran = server.terminal_run(Parameters(run("~"))).await;
            assert_ne!(ran.is_error, Some(true), "{}", said(&ran));
            assert_eq!(cwd_asked(&asked), Some(PathBuf::from(home)));
        }

        for (cwd, why) in [
            ("web", "must be an absolute path"),
            ("/no/such/folder", "is not a folder"),
        ] {
            let refused = server.terminal_run(Parameters(run(cwd))).await;
            assert_eq!(refused.is_error, Some(true), "{cwd}: {}", said(&refused));
            assert!(said(&refused).contains(why), "{cwd}: {}", said(&refused));
        }
        assert!(
            asked.recv_timeout(Duration::from_millis(300)).is_err(),
            "the app was asked to run in a folder that isn't one"
        );
    }

    /// Without `project`, a terminal or app tool names the folder the agent works in, so the app
    /// uses the window that has it open rather than whichever window is in front; with one, it
    /// names only the project.
    #[tokio::test]
    async fn app_tools_name_the_agents_folder() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let asked = fake_app_answering(
            &server.paths,
            vec![json!({"text": ""}), json!({"text": ""})],
        );
        let read = |project: Option<String>| TerminalReadArgs {
            tab: None,
            pane: None,
            lines: None,
            project,
        };
        server.terminal_read(Parameters(read(None))).await;
        let request = asked
            .recv_timeout(Duration::from_secs(5))
            .expect("the app was asked");
        assert_eq!(request.project, None);
        assert_eq!(request.agent_folder, crate::project_dir(None));
        assert!(request.agent_folder.is_some(), "the tests run in a folder");

        server
            .terminal_read(Parameters(read(Some("/code/shop".into()))))
            .await;
        let request = asked
            .recv_timeout(Duration::from_secs(5))
            .expect("the app was asked");
        assert_eq!(request.project, Some(PathBuf::from("/code/shop")));
        assert_eq!(request.agent_folder, None);
    }

    /// The next op the stand-in app was asked for. It is waited for, but not for ever: a tool that
    /// didn't go to the app fails the test instead of hanging it.
    fn next(asked: &Receiver<Value>) -> Value {
        asked
            .recv_timeout(Duration::from_secs(5))
            .expect("the app was asked for an op")
    }

    #[tokio::test]
    async fn a_running_app_answers_every_canvas_tool() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = dir.path().join("project");
        write_board(&board_file(&project), 3);
        let asked = fake_app(
            &server.paths,
            vec![
                json!({"ok": true, "elements": [{"id": "7", "type": "sticky"}]}),
                json!({"ok": false, "error": "add: the engine said no"}),
                json!({"ok": true, "result": {"id": "9"}}),
                json!({"ok": true, "commands": [{"id": "a.b", "label": "A B", "params": "{}"}]}),
            ],
        );

        // The app's answer to `get`, though the native canvas is on and the file could be read:
        // the app is the one that has the canvas.
        let got = server
            .canvas_get_tool(WindowArgs {
                project: project_arg(&project),
            })
            .await;
        assert_eq!(answer(&got)["elements"].as_array().unwrap().len(), 1);
        assert_eq!(next(&asked), json!({"op": "get"}));

        // An op the app answers with `{ok: false, error}` is the tool's error, in the engine's words.
        let failed = server
            .canvas_add_elements(Parameters(CanvasElementsArgs {
                elements: vec![json!({"type": "rectangle"})],
                project: project_arg(&project),
            }))
            .await;
        assert_eq!(failed.is_error, Some(true), "{}", said(&failed));
        assert_eq!(said(&failed), "add: the engine said no");
        assert_eq!(next(&asked)["op"], json!("add"));

        let ran = server
            .canvas_command_tool(CanvasCommandArgs {
                command: "board.sticky".into(),
                params: Some(json!({"x": 0, "y": 0})),
                project: project_arg(&project),
            })
            .await;
        assert_eq!(answer(&ran)["result"]["id"], json!("9"));
        let op = json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}});
        assert_eq!(next(&asked), op);

        let listed = server
            .canvas_commands(Parameters(CanvasCommandsArgs {
                filter: None,
                project: project_arg(&project),
            }))
            .await;
        assert_eq!(answer(&listed)["commands"][0]["id"], json!("a.b"));
        assert_eq!(next(&asked), json!({"op": "commands"}));

        // None of it went to the file behind the app's back.
        assert_eq!(layers_in(&board_file(&project)).len(), 3);
    }

    /// MCP-8: the file or folder an engine command names goes to the canvas as the agent means
    /// it: `~` for the home folder, and a relative one from the agent's folder. As written it was
    /// the app's folder's (`/` for Sawit.app) with the app running, and `~/…` was no folder at
    /// all. What isn't a file is left as it is: SVG path data, a layer's id, a share link, a
    /// number.
    #[tokio::test]
    async fn canvas_command_sends_the_files_it_names_as_the_agent_means_them() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let asked = fake_app(&server.paths, Vec::new());
        let here = std::env::current_dir().unwrap();
        let sent = |command: &str, params: Value| {
            let run = server.canvas_command_tool(CanvasCommandArgs {
                command: command.into(),
                params: Some(params),
                project: project_arg(&project),
            });
            async {
                let ran = run.await;
                assert_ne!(ran.is_error, Some(true), "{}", said(&ran));
                next(&asked)["params"].clone()
            }
        };

        let params = sent("file.exportSvg", json!({"path": " rel/page.svg "})).await;
        assert_eq!(params["path"], json!(here.join("rel/page.svg")));
        let params = sent("file.exportLayers", json!({"dir": "out", "ids": [1]})).await;
        assert_eq!(params, json!({"dir": here.join("out"), "ids": [1]}));
        if let Some(home) = std::env::var_os("HOME") {
            let params = sent("image.place", json!({"path": "~/Desktop/a.png"})).await;
            let placed = Path::new(&home).join("Desktop").join("a.png");
            assert_eq!(params["path"], json!(placed));
        }
        for (command, params) in [
            ("vector.create", json!({"path": "M0 0 L10 0", "x": 0})),
            ("board.ink", json!({"path": "M0 0 L10 10"})),
            ("text.onPath", json!({"path": "12"})),
            ("library.add", json!({"path": "https://share.example/f/1"})),
            ("comment.next", json!({"dir": 1})),
            ("file.exportSvg", json!({"path": "/abs/page.svg"})),
        ] {
            assert_eq!(sent(command, params.clone()).await, params, "{command}");
        }
        let batch = json!({"commands": [
            {"command": "file.exportSvg", "params": {"path": "b.svg"}},
            {"command": "vector.create", "params": {"path": "M0 0"}},
            {"command": "file.exportSvg", "params": {"$step": 0}},
        ]});
        let steps = sent("command.batch", batch).await["commands"].clone();
        assert_eq!(steps[0]["params"]["path"], json!(here.join("b.svg")));
        assert_eq!(steps[1]["params"]["path"], json!("M0 0"));
        assert_eq!(steps[2]["params"], json!({"$step": 0}));
    }

    /// Every command [`FILE_PARAMS`] names is one canvas_command runs, and takes that parameter:
    /// a command the engine renamed or dropped would leave its files read from the app's folder.
    #[test]
    fn the_commands_that_take_a_file_are_the_engines() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let reply = ops::run(&mut board, &json!({"op": "commands"}));
        let commands = reply["commands"].as_array().unwrap();
        for (id, key) in FILE_PARAMS {
            let command = commands.iter().find(|c| c["id"] == json!(id));
            let params = command.map(|c| c["params"].as_str().unwrap_or_default());
            let params = params.unwrap_or_else(|| panic!("{id} is no command"));
            assert!(params.contains(key), "{id} takes no {key}: {params}");
        }
    }

    #[tokio::test]
    async fn the_other_canvas_tools_edit_the_stored_canvas_while_the_app_is_closed() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let file = board_file(&project);

        // Two boxes and an arrow between them, with the agent's own ids.
        let added = server
            .canvas_add_elements(Parameters(CanvasElementsArgs {
                elements: vec![
                    json!({"type": "rectangle", "id": "api", "x": 0, "y": 0, "width": 160, "height": 60, "label": {"text": "API"}}),
                    json!({"type": "rectangle", "id": "db", "x": 300, "y": 0, "width": 160, "height": 60, "label": {"text": "DB"}}),
                    json!({"type": "arrow", "id": "calls", "x": 160, "y": 30, "start": {"id": "api"}, "end": {"id": "db"}}),
                ],
                project: project_arg(&project),
            }))
            .await;
        assert_eq!(answer(&added)["added"], json!(3), "{}", said(&added));
        assert_eq!(layers_in(&file).len(), 3);

        // The ids come back in the next call, which opens the file again.
        let updated = server
            .canvas_update(Parameters(CanvasUpdateArgs {
                elements: vec![json!({"id": "db", "text": "Database"})],
                project: project_arg(&project),
            }))
            .await;
        assert_eq!(answer(&updated)["updated"], json!(1), "{}", said(&updated));
        let layers = layers_in(&file);
        let texts: Vec<&str> = layers.iter().filter_map(|l| l["text"].as_str()).collect();
        assert!(
            texts.contains(&"Database") && texts.contains(&"API"),
            "{texts:?}"
        );
        let arrow = layers
            .iter()
            .find(|l| l["type"] == json!("connector"))
            .unwrap();
        assert!(
            arrow["from"].is_string() && arrow["to"].is_string(),
            "{arrow}"
        );

        // A diagram from Mermaid is drawn as shapes the next call can edit.
        let drawn = server
            .canvas_add_mermaid(Parameters(CanvasMermaidArgs {
                source: "graph TD; A-->B".into(),
                x: Some(0.0),
                y: Some(400.0),
                project: project_arg(&project),
            }))
            .await;
        assert!(
            answer(&drawn)["added"].as_u64().unwrap() >= 3,
            "{}",
            said(&drawn)
        );
        let after_mermaid = layers_in(&file).len();
        assert!(after_mermaid > 3, "{after_mermaid}");

        // A command without parameters.
        let zoomed = server
            .canvas_command_tool(CanvasCommandArgs {
                command: "view.zoomFit".into(),
                params: None,
                project: project_arg(&project),
            })
            .await;
        assert_ne!(zoomed.is_error, Some(true), "{}", said(&zoomed));

        let deleted = server
            .canvas_delete(Parameters(CanvasDeleteArgs {
                ids: vec!["calls".into(), "nope".into()],
                project: project_arg(&project),
            }))
            .await;
        let deleted = answer(&deleted);
        assert_eq!(deleted["deleted"], json!(1), "{deleted}");
        assert_eq!(deleted["missing"], json!(["nope"]), "{deleted}");
        assert_eq!(layers_in(&file).len(), after_mermaid - 1);

        // Clearing the canvas keeps what it held, in the `.bak` beside it.
        let cleared = server
            .canvas_clear(Parameters(WindowArgs {
                project: project_arg(&project),
            }))
            .await;
        assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
        assert!(layers_in(&file).is_empty());
        let kept = std::fs::read(file.with_extension("figcraft.bak")).unwrap();
        let kept = BoardDoc::from_bytes(&kept).unwrap();
        let kept = ops::run(&mut { kept }, &json!({"op": "get"}));
        assert_eq!(
            kept["elements"].as_array().unwrap().len(),
            after_mermaid - 1
        );
    }

    #[tokio::test]
    async fn parallel_calls_each_land_in_the_stored_canvas() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());

        // An agent's parallel tool calls: each opens the file, adds its box and saves.
        let calls = (0..8).map(|n| {
            server.canvas_add_elements(Parameters(CanvasElementsArgs {
                elements: vec![json!({"type": "rectangle", "id": format!("box-{n}"), "x": n * 200, "y": 0, "width": 100, "height": 60})],
                project: project_arg(&project),
            }))
        });
        for result in futures::future::join_all(calls).await {
            assert_ne!(result.is_error, Some(true), "{}", said(&result));
        }

        assert_eq!(layers_in(&board_file(&project)).len(), 8);
    }

    #[tokio::test]
    async fn canvas_load_takes_a_figcraft_file_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = dir.path().join("project");
        let file = board_file(&project);
        write_board(&file, 1);
        let other = dir.path().join("elsewhere").join("board.FIGCRAFT");
        write_board(&other, 3);
        let other_bytes = std::fs::read(&other).unwrap();
        let load = |path: &Path| CanvasLoadArgs {
            path: Some(path.display().to_string()),
            scene: None,
            project: project_arg(&project),
        };

        let loaded = server.canvas_load(Parameters(load(&other))).await;
        assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 3);
        assert_eq!(layers_in(&file).len(), 3);
        assert_eq!(std::fs::read(&other).unwrap(), other_bytes);

        // An Excalidraw file is still read here and sent as a scene.
        let old = dir.path().join("old.excalidraw");
        std::fs::write(&old, OLD_BOARD).unwrap();
        let loaded = server.canvas_load(Parameters(load(&old))).await;
        assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 9);
        assert_eq!(layers_in(&file).len(), 9);

        // A .figcraft file that isn't there is an error that changes nothing.
        let before = std::fs::read(&file).unwrap();
        let missing = server
            .canvas_load(Parameters(load(&dir.path().join("nope.figcraft"))))
            .await;
        assert_eq!(missing.is_error, Some(true), "{}", said(&missing));
        assert_eq!(std::fs::read(&file).unwrap(), before);

        // Neither is no load at all.
        let nothing = server
            .canvas_load(Parameters(CanvasLoadArgs {
                path: None,
                scene: None,
                project: project_arg(&project),
            }))
            .await;
        assert_eq!(said(&nothing), "Give a path or a scene.");
    }

    /// Runs `call` on a runtime of its own, on a thread of its own, and gives what it answered:
    /// `None` when it hadn't answered within 5 s (the thread is left as it is). For a call whose
    /// read could block for ever, so that a test of it fails rather than hangs.
    fn within_seconds<F>(call: F) -> Option<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = tx.send(runtime.block_on(call));
        });
        rx.recv_timeout(Duration::from_secs(5)).ok()
    }

    /// CANVAS-CORE-6, for an agent's load: `canvas_load` of a file that is a named pipe, or a link
    /// a cloned repository holds to `/dev/zero`, never answered. The canvas's engine read a
    /// `.figcraft` file (with the app closed, in the blocking task that holds every other canvas
    /// call back; with it open, on its UI thread), and the tool itself read any other, on the
    /// server's async thread. Each is refused at once now, and the canvas is as it was.
    #[test]
    fn canvas_load_of_a_pipe_or_a_device_is_refused_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let file = board_file(&project);
        write_board(&file, 1);
        let before = std::fs::read(&file).unwrap();
        let refused_at_once = |paths: &[PathBuf]| {
            let failed: Vec<String> = (paths.iter())
                .filter_map(|path| {
                    let (server, project) = (server.clone(), project_arg(&project));
                    let path = path.display().to_string();
                    let load = CanvasLoadArgs {
                        path: Some(path.clone()),
                        scene: None,
                        project,
                    };
                    let call = async move { server.canvas_load(Parameters(load)).await };
                    let Some(result) = within_seconds(call) else {
                        return Some(format!("{path}: no answer within 5 s"));
                    };
                    let error = said(&result);
                    let refused = result.is_error == Some(true)
                        && error.contains(&path)
                        && error.contains("not a regular file");
                    (!refused).then(|| format!("{path}: {error}"))
                })
                .collect();
            assert!(failed.is_empty(), "{}", failed.join("\n"));
        };
        let pipes =
            ["board.figcraft", "scene.excalidraw", "scene.json"].map(|n| dir.path().join(n));
        for pipe in &pipes {
            let made = std::process::Command::new("mkfifo").arg(pipe).status();
            assert!(made.unwrap().success(), "mkfifo {}", pipe.display());
        }
        // The pipes first: a device that never ends is read only once they are refused.
        refused_at_once(&pipes);
        let zeros = ["zero.figcraft", "zero.excalidraw"].map(|n| dir.path().join(n));
        for zero in &zeros {
            std::os::unix::fs::symlink("/dev/zero", zero).unwrap();
        }
        refused_at_once(&zeros);
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }

    /// MCP-6: canvas_clear keeps the board it cleared as `canvas.figcraft.bak`, and canvas_load
    /// brings it back from there. It read the backup as Excalidraw JSON ("stream did not contain
    /// valid UTF-8"), so no tool could restore a cleared board.
    #[tokio::test]
    async fn canvas_load_brings_back_the_board_a_clear_kept() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let file = board_file(&project);
        write_board(&file, 3);

        let cleared = server
            .canvas_clear(Parameters(WindowArgs {
                project: project_arg(&project),
            }))
            .await;
        assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
        assert!(layers_in(&file).is_empty());

        let backup = file.with_extension("figcraft.bak");
        let loaded = server
            .canvas_load(Parameters(CanvasLoadArgs {
                path: Some(backup.display().to_string()),
                scene: None,
                project: project_arg(&project),
            }))
            .await;
        assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 3);
        let texts = texts_in(&file);
        assert!(
            (0..3).all(|n| texts.contains(&format!("note {n}"))),
            "{texts:?}"
        );
    }

    /// MCP-7: canvas_load reads an Excalidraw file's path as it reads a `.figcraft` file's:
    /// trimmed, `~` for the home folder, and relative to the agent's folder. It read the path as
    /// written: a space at its end, or `~/`, was "No such file or directory".
    #[tokio::test]
    async fn canvas_load_reads_an_import_path_as_it_reads_a_figcraft_path() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let old = dir.path().join("old.excalidraw");
        std::fs::write(&old, OLD_BOARD).unwrap();
        let load = |path: String| CanvasLoadArgs {
            path: Some(path),
            scene: None,
            project: project_arg(&project),
        };

        let loaded = server
            .canvas_load(Parameters(load(format!(" {} ", old.display()))))
            .await;
        assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 9);

        if let Some(home) = std::env::var_os("HOME") {
            let name = "sawit-test-no-such-board.excalidraw";
            let missing = server
                .canvas_load(Parameters(load(format!("~/{name}"))))
                .await;
            assert_eq!(missing.is_error, Some(true));
            let expanded = Path::new(&home).join(name);
            assert!(
                said(&missing).contains(&expanded.display().to_string()),
                "{}",
                said(&missing)
            );
        }
    }

    #[tokio::test]
    async fn canvas_commands_lists_every_command_and_filters_with_their_parameters() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let list = |filter: Option<&str>| {
            server.canvas_commands(Parameters(CanvasCommandsArgs {
                filter: filter.map(str::to_string),
                project: project_arg(&project),
            }))
        };

        // The list reaches the agent whole, with no cut: the board's own commands are the last
        // that the engine registers, so a list that doesn't fit would lose exactly those.
        let all = list(None).await;
        assert!(!said(&all).contains("truncated"), "{}", said(&all).len());
        let all = answer(&all);
        let commands = all["commands"].as_array().unwrap();
        assert!(commands.len() > 500, "{}", commands.len());
        let ids: Vec<&str> = commands.iter().filter_map(|c| c["id"].as_str()).collect();
        assert!(ids.contains(&"board.sticky") && ids.contains(&"edit.undo"));
        // It is what `canvas_command` runs: what Sawit does itself isn't there.
        assert!(!ids.contains(&"file.save"));
        assert!(
            commands
                .iter()
                .all(|c| c["label"].is_string() && c.get("params").is_none())
        );

        // With a filter, the commands that match come with the parameters they take.
        let board = answer(&list(Some(" BOARD. ")).await);
        let board = board["commands"].as_array().unwrap();
        assert!(!board.is_empty() && board.len() < 50, "{}", board.len());
        let sticky = board
            .iter()
            .find(|c| c["id"] == json!("board.sticky"))
            .unwrap();
        assert!(
            sticky["params"].as_str().unwrap().contains("text"),
            "{sticky}"
        );
        // A part of a label counts too, and a filter that matches nothing is an empty list.
        let by_label = answer(&list(Some("sticky note")).await);
        assert!(
            by_label["commands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == json!("board.sticky"))
        );
        assert_eq!(
            answer(&list(Some("no such command")).await)["commands"],
            json!([])
        );
        // Blank is no filter.
        assert_eq!(answer(&list(Some("  ")).await), all);
    }

    #[tokio::test]
    async fn a_canvas_that_can_not_be_saved_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = dir.path().join("project");
        let file = board_file(&project);
        write_board(&file, 1);
        let before = std::fs::read(&file).unwrap();
        let folder = file.parent().unwrap();
        let writable =
            |mode| std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode));
        writable(0o555).unwrap();
        if std::fs::write(folder.join("probe"), b"x").is_ok() {
            // Whoever may write to a folder that is read-only (root) can't make a save fail this way.
            writable(0o755).unwrap();
            return;
        }

        let result = server
            .canvas_command_tool(CanvasCommandArgs {
                command: "board.sticky".into(),
                params: Some(json!({"x": 0, "y": 0})),
                project: project_arg(&project),
            })
            .await;
        writable(0o755).unwrap();

        // Not a success with the sticky in the reply: it is not in the file.
        assert_eq!(result.is_error, Some(true), "{}", said(&result));
        assert!(
            said(&result).contains("couldn't be saved"),
            "{}",
            said(&result)
        );
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }

    /// Plan 7 final review I-1 and T5-5: a canvas file written by another since this call read it
    /// (a second agent's `sawit-mcp`, whose turns this process doesn't take) is never saved over,
    /// which would drop that edit though its call answered ok. The save is refused in FigCraft's
    /// sentence, and the file keeps the other's bytes; read again, the call goes through.
    #[test]
    fn a_canvas_saved_by_another_since_it_was_read_is_not_saved_over() {
        let dir = tempfile::tempdir().unwrap();
        let file = board_file(&project_in(dir.path()));
        write_board(&file, 1);
        // Read as `headless` reads it: stamped, then opened.
        let read = Stamp::of(&file);
        let mut board = store::open(&file).unwrap();
        // The other process saves its edit in between.
        write_board(&file, 2);
        let theirs = std::fs::read(&file).unwrap();

        let sticky =
            json!({"op": "command", "command": "board.sticky", "params": {"x": 900, "y": 0}});
        let refused = run_and_save(&mut board, &file, read, &sticky).unwrap_err();
        assert!(
            refused.contains("the file changed on disk since it was opened or saved here"),
            "{refused}"
        );
        assert_eq!(std::fs::read(&file).unwrap(), theirs);

        let read = Stamp::of(&file);
        let mut board = store::open(&file).unwrap();
        run_and_save(&mut board, &file, read, &sticky).unwrap();
        assert_eq!(layers_in(&file).len(), 3);
    }

    /// The texts of the stickies and other text layers on the canvas stored in `file`.
    fn texts_in(file: &Path) -> std::collections::HashSet<String> {
        let layers = layers_in(file);
        let texts = layers.iter().filter_map(|layer| layer["text"].as_str());
        texts.map(str::to_string).collect()
    }

    /// MCP-1: two agents' `sawit-mcp` processes edit one canvas while Sawit is closed. Each call's
    /// answer must say what happened to its edit: ok when it is on the canvas, an error when it
    /// isn't. Their saves raced: a call answered ok while the other's save went over its edit,
    /// and an error while its edit was the one on disk. Threads stand in for the processes here,
    /// each taking only the turns that hold across processes ([`headless_turn`]).
    #[test]
    fn agents_in_two_processes_get_true_answers_and_lose_no_edit() {
        let dir = tempfile::tempdir().unwrap();
        let file = board_file(&project_in(dir.path()));
        let lock = dir.path().join("home").join(CANVAS_LOCK);
        let (writers, calls) = (4, 12);
        let answers: Vec<(String, Result<Value, String>)> = std::thread::scope(|scope| {
            let running: Vec<_> = (0..writers)
                .map(|writer| {
                    let (file, lock) = (&file, &lock);
                    scope.spawn(move || {
                        (0..calls)
                            .map(|call| {
                                let text = format!("w{writer}-n{call}");
                                let sticky = json!({"type": "sticky", "x": call * 250, "y": writer * 250, "text": text});
                                let op = json!({"op": "add", "elements": [sticky]});
                                (text, headless_turn(file, lock, &op))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            running
                .into_iter()
                .flat_map(|writer| writer.join().unwrap())
                .collect()
        });

        let on_canvas = texts_in(&file);
        let mut lies = Vec::new();
        for (text, answer) in &answers {
            match answer {
                Ok(_) if !on_canvas.contains(text) => lies.push(format!("{text}: ok, not saved")),
                Err(e) if on_canvas.contains(text) => lies.push(format!("{text}: saved, yet {e}")),
                _ => {}
            }
        }
        assert!(lies.is_empty(), "{} wrong answers: {lies:#?}", lies.len());
        // Taking turns, no call has anything to refuse.
        let failed: Vec<_> = answers.iter().filter(|(_, a)| a.is_err()).collect();
        assert!(
            failed.is_empty(),
            "{} calls failed: {failed:#?}",
            failed.len()
        );
        assert_eq!(on_canvas.len(), writers * calls);
    }

    /// MCP-1: something that takes no turns (Sawit as it starts, another program) saves the
    /// stored canvas while a call runs. The call's save would drop that edit, so it is refused,
    /// and the call runs again on what is on disk now: both edits land, and the answer is ok.
    /// When the file changes every time, nothing of the call's is saved, and the error tells the
    /// agent what to do, not to close a tab it doesn't have.
    #[test]
    fn a_call_runs_again_on_a_canvas_saved_under_it_and_says_when_it_gave_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = board_file(&project_in(dir.path()));
        write_board(&file, 1);
        let sticky = |text: &str| json!({"op": "add", "elements": [{"type": "sticky", "x": 600, "y": 0, "text": text}]});
        // Another program's save: the board with one more sticky.
        let save_theirs = |text: &str| {
            let mut board = store::open(&file).unwrap();
            let reply = ops::run(&mut board, &sticky(text));
            assert_eq!(reply["ok"], true, "{reply}");
            std::fs::write(&file, board.to_bytes().unwrap()).unwrap();
        };

        let mut reads = 0;
        let once = edit_stored(&file, &sticky("ours"), || {
            reads += 1;
            if reads == 1 {
                save_theirs("theirs");
            }
        });
        assert!(once.is_ok(), "{once:?}");
        assert_eq!(reads, 2);
        let texts = texts_in(&file);
        assert!(
            texts.contains("ours") && texts.contains("theirs"),
            "{texts:?}"
        );

        let mut reads = 0;
        let every_time = edit_stored(&file, &sticky("never"), || {
            reads += 1;
            save_theirs(&format!("theirs {reads}"));
        });
        let error = every_time.unwrap_err();
        assert_eq!(reads, CANVAS_ATTEMPTS);
        assert!(error.starts_with("Nothing was saved"), "{error}");
        assert!(error.contains("Call it again"), "{error}");
        assert!(!error.contains("tab"), "{error}");
        let texts = texts_in(&file);
        assert!(!texts.contains("never"), "{texts:?}");
        assert!(
            texts.contains(&format!("theirs {CANVAS_ATTEMPTS}")),
            "{texts:?}"
        );
    }

    /// The canvas lock is let go when a call ends, and a link at its name is never followed.
    #[test]
    fn the_canvas_lock_is_let_go_and_never_taken_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("home").join(CANVAS_LOCK);
        let held = lock_stored_canvas(&lock).unwrap();
        // Held, another open file of it can't have it: as another process couldn't.
        let other = std::fs::File::open(&lock).unwrap();
        assert!(matches!(
            other.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(held);
        other.try_lock().unwrap();
        drop(other);

        std::fs::remove_file(&lock).unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "the user's").unwrap();
        std::os::unix::fs::symlink(&victim, &lock).unwrap();
        let refused = lock_stored_canvas(&lock).unwrap_err();
        assert!(refused.contains("it is a link"), "{refused}");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "the user's");
    }

    /// Plan 7 final review T5-6: with Sawit closed every call opens the canvas afresh, so a load
    /// can't be undone as it can in the app. The canvas it replaces is kept as `<file>.bak`, as a
    /// clear keeps one, so one mistaken load doesn't lose the board. A canvas with nothing on it
    /// has nothing to keep.
    #[tokio::test]
    async fn an_offline_load_keeps_the_canvas_it_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let file = board_file(&project);
        let bak = file.with_extension("figcraft.bak");
        write_board(&file, 2);
        let replaced = std::fs::read(&file).unwrap();
        let other = dir.path().join("other.figcraft");
        write_board(&other, 1);
        let load = |path: &Path| CanvasLoadArgs {
            path: Some(path.display().to_string()),
            scene: None,
            project: project_arg(&project),
        };

        let loaded = server.canvas_load(Parameters(load(&other))).await;
        assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 1);
        assert_eq!(layers_in(&file).len(), 1);
        assert_eq!(std::fs::read(&bak).unwrap(), replaced);

        // Over a canvas with nothing on it.
        std::fs::remove_file(&bak).unwrap();
        write_board(&file, 0);
        let loaded = server.canvas_load(Parameters(load(&other))).await;
        assert_eq!(answer(&loaded)["elements"].as_array().unwrap().len(), 1);
        assert!(!bak.exists());
    }

    /// The bytes of the backups beside the canvas file `file`: `<name>.bak` and the older ones.
    fn backups_of(file: &Path) -> Vec<Vec<u8>> {
        let dir = file.parent().unwrap();
        let name = format!("{}.bak", file.file_name().unwrap().to_string_lossy());
        let mut kept: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&name)
            })
            .collect();
        kept.sort();
        kept.iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect()
    }

    /// CANVAS-CORE-3: with Sawit closed every call opens the canvas afresh, so its backups are all
    /// that is left of a board an agent clears or loads over. Starting over twice (a clear, a
    /// drawing and a clear; or a load, then another) kept one `.bak`, of the step between, and
    /// the user's board was in no file. It is still kept.
    #[tokio::test]
    async fn starting_over_twice_with_sawit_closed_keeps_the_users_board() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let file = board_file(&project);
        let window = || WindowArgs {
            project: project_arg(&project),
        };
        write_board(&file, 3);
        let users = std::fs::read(&file).unwrap();

        // A clear, a drawing and a clear.
        let cleared = server.canvas_clear(Parameters(window())).await;
        assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
        let drawn = server
            .canvas_add_elements(Parameters(CanvasElementsArgs {
                elements: vec![json!({"type": "rectangle", "x": 0, "y": 0})],
                project: project_arg(&project),
            }))
            .await;
        assert_ne!(drawn.is_error, Some(true), "{}", said(&drawn));
        let cleared = server.canvas_clear(Parameters(window())).await;
        assert_ne!(cleared.is_error, Some(true), "{}", said(&cleared));
        assert!(layers_in(&file).is_empty());
        assert!(
            backups_of(&file).contains(&users),
            "the user's board is gone"
        );

        // A load, then another.
        write_board(&file, 3);
        let users = std::fs::read(&file).unwrap();
        let (one, two) = (
            dir.path().join("one.figcraft"),
            dir.path().join("two.figcraft"),
        );
        write_board(&one, 1);
        write_board(&two, 2);
        for other in [&one, &two] {
            let loaded = server
                .canvas_load(Parameters(CanvasLoadArgs {
                    path: Some(other.display().to_string()),
                    scene: None,
                    project: project_arg(&project),
                }))
                .await;
            assert_ne!(loaded.is_error, Some(true), "{}", said(&loaded));
        }
        assert_eq!(layers_in(&file).len(), 2);
        assert!(
            backups_of(&file).contains(&users),
            "the user's board is gone"
        );
    }

    /// CANVAS-CORE-3: a load the changed-on-disk guard refuses changes nothing, the backups
    /// included. Its `.bak` used to be made first, from the other process's file.
    #[test]
    fn a_refused_load_leaves_the_backups_as_they_were() {
        let dir = tempfile::tempdir().unwrap();
        let file = board_file(&project_in(dir.path()));
        write_board(&file, 1);
        let bak = file.with_extension("figcraft.bak");
        write_board(&bak, 4);
        let before = backups_of(&file);
        let read = Stamp::of(&file);
        let mut board = store::open(&file).unwrap();
        write_board(&file, 2);
        let theirs = std::fs::read(&file).unwrap();

        let other = dir.path().join("other.figcraft");
        write_board(&other, 3);
        let load = json!({"op": "replace", "path": other});
        let refused = run_and_save(&mut board, &file, read, &load).unwrap_err();
        assert!(refused.contains("changed on disk"), "{refused}");
        assert_eq!(std::fs::read(&file).unwrap(), theirs);
        assert!(backups_of(&file) == before, "the backup was replaced");
    }

    /// CANVAS-CORE-1: a cloned project can hold a link at `canvas.figcraft.bak` to any file of
    /// the user's. A load with Sawit closed keeps the canvas it replaces in place of the link,
    /// never through it.
    #[tokio::test]
    async fn an_offline_load_never_writes_through_a_link_at_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = project_in(dir.path());
        let file = board_file(&project);
        let bak = file.with_extension("figcraft.bak");
        write_board(&file, 2);
        let replaced = std::fs::read(&file).unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "the user's").unwrap();
        std::os::unix::fs::symlink(&victim, &bak).unwrap();
        let other = dir.path().join("other.figcraft");
        write_board(&other, 1);

        let loaded = server
            .canvas_load(Parameters(CanvasLoadArgs {
                path: Some(other.display().to_string()),
                scene: None,
                project: project_arg(&project),
            }))
            .await;
        assert_ne!(loaded.is_error, Some(true), "{}", said(&loaded));
        assert_eq!(layers_in(&file).len(), 1);
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "the user's");
        assert!(
            std::fs::symlink_metadata(&bak)
                .unwrap()
                .file_type()
                .is_file()
        );
        assert_eq!(std::fs::read(&bak).unwrap(), replaced);
    }

    #[test]
    fn a_failed_reply_is_an_error_and_any_other_is_the_answer() {
        assert_eq!(
            op_reply(json!({"ok": false, "error": "add: no"})),
            Err("add: no".to_string())
        );
        assert_eq!(
            op_reply(json!({"ok": false})),
            Err("The canvas op failed.".to_string())
        );
        let fine = json!({"ok": true, "added": 1});
        assert_eq!(op_reply(fine.clone()), Ok(fine));
        // The old canvas's page leaves `ok` out of what it answers.
        let old = json!({"count": 2, "elements": []});
        assert_eq!(op_reply(old.clone()), Ok(old));
        assert_eq!(op_reply(Value::Null), Ok(Value::Null));
    }

    #[test]
    fn figcraft_files_are_told_by_their_extension() {
        assert!(is_figcraft("/a/b/board.figcraft"));
        assert!(is_figcraft("board.FigCraft"));
        assert!(is_figcraft("  ~/board.figcraft "));
        assert!(!is_figcraft("/a/b/board.excalidraw"));
        assert!(!is_figcraft("figcraft"));
        assert!(!is_figcraft("/a/figcraft/board"));
        // The backups Sawit keeps of a canvas file are FigCraft files too; an export of one isn't.
        assert!(is_figcraft("/p/.nyawit/canvas.figcraft.bak"));
        assert!(is_figcraft("/p/.nyawit/canvas.figcraft.bak.3"));
        assert!(is_figcraft("/p/.nyawit/canvas.figcraft.lossy.bak"));
        assert!(!is_figcraft("/p/canvas.figcraft.svg"));
        assert!(!is_figcraft("/p/canvas.figcraft.bak.x"));
    }

    #[test]
    fn a_path_for_the_app_is_absolute() {
        assert_eq!(absolute_path("/a/b.figcraft"), "/a/b.figcraft");
        let relative = absolute_path("b.figcraft");
        assert!(Path::new(&relative).is_absolute(), "{relative}");
        assert!(relative.ends_with("/b.figcraft"), "{relative}");
        if let Some(home) = std::env::var_os("HOME") {
            let home = Path::new(&home).join("b.figcraft");
            assert_eq!(absolute_path("~/b.figcraft"), home.to_string_lossy());
        }
    }

    #[tokio::test]
    async fn an_export_never_overwrites_the_canvas_file() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = dir.path().join("project");
        let file = board_file(&project);
        write_board(&file, 1);
        let was = std::fs::read(&file).unwrap();

        // The canvas_command path: the guard is the op's own, so it holds with no app running.
        let refused = server
            .canvas_command_tool(CanvasCommandArgs {
                command: "file.exportSvg".into(),
                params: Some(json!({"path": file})),
                project: project_arg(&project),
            })
            .await;
        assert_eq!(refused.is_error, Some(true), "{}", said(&refused));
        assert_eq!(
            said(&refused),
            "file.exportSvg would overwrite a canvas file"
        );
        assert_eq!(std::fs::read(&file).unwrap(), was);

        // The canvas_export path, for every canvas name and both formats.
        for name in [
            "canvas.figcraft",
            "canvas.excalidraw",
            "canvas.figcraft.bak",
        ] {
            for format in ["svg", "png"] {
                let target = file.with_file_name(name);
                let refused = server
                    .canvas_export(Parameters(CanvasExportArgs {
                        format: Some(format.into()),
                        output_path: Some(target.display().to_string()),
                        project: project_arg(&project),
                    }))
                    .await;
                assert_eq!(refused.is_error, Some(true), "{name}: {}", said(&refused));
                assert!(said(&refused).contains("would overwrite a canvas file"));
            }
        }
        assert_eq!(std::fs::read(&file).unwrap(), was);
        assert!(!file.with_file_name("canvas.excalidraw").exists());

        // The canvas is still there, with its sticky.
        let got = server
            .canvas_get_tool(WindowArgs {
                project: project_arg(&project),
            })
            .await;
        assert_eq!(answer(&got)["elements"].as_array().unwrap().len(), 1);

        // An export to an image path still writes.
        for (format, name) in [("svg", "page.svg"), ("png", "page.png")] {
            let out = dir.path().join(name);
            let done = server
                .canvas_export(Parameters(CanvasExportArgs {
                    format: Some(format.into()),
                    output_path: Some(out.display().to_string()),
                    project: project_arg(&project),
                }))
                .await;
            assert_ne!(done.is_error, Some(true), "{}", said(&done));
            assert!(!std::fs::read(&out).unwrap().is_empty());
        }
    }

    /// MCP-11: canvas_export writes where every tool that writes a file writes: an absolute path
    /// (or `~/…`), or with none (or a blank one) the project's exports folder. A relative path was
    /// written in sawit-mcp's folder and reported as given, and "PNG" was an unknown format.
    #[tokio::test]
    async fn canvas_export_reads_its_path_and_format_as_every_tool_does() {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = dir.path().join("project");
        write_board(&board_file(&project), 1);
        let export = |format: &str, output_path: &str| CanvasExportArgs {
            format: Some(format.into()),
            output_path: Some(output_path.into()),
            project: project_arg(&project),
        };

        // Through a file, so that nothing can be written under the crate whatever happens.
        let refused = server
            .canvas_export(Parameters(export("svg", "Cargo.toml/out.svg")))
            .await;
        assert_eq!(refused.is_error, Some(true), "{}", said(&refused));
        assert!(
            said(&refused).contains("must be absolute"),
            "{}",
            said(&refused)
        );

        for (format, name) in [("PNG", "page.png"), (" Svg ", "page.svg")] {
            let out = dir.path().join(name);
            let done = server
                .canvas_export(Parameters(export(format, &out.display().to_string())))
                .await;
            assert_ne!(done.is_error, Some(true), "{format}: {}", said(&done));
            assert!(!std::fs::read(&out).unwrap().is_empty());
        }

        let blank = server.canvas_export(Parameters(export("svg", "  "))).await;
        assert_ne!(blank.is_error, Some(true), "{}", said(&blank));
        let exports = project.join(".nyawit").join("exports");
        assert!(
            said(&blank).contains(&exports.display().to_string()),
            "{}",
            said(&blank)
        );
    }

    /// The SVG `canvas_export` writes, with no app running, for a canvas of one sticky that says
    /// `text`.
    async fn exported_sticky(text: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = dir.path().join("project");
        let file = board_file(&project);
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let sticky = json!({"x": 0, "y": 0, "text": text});
        board.execute("board.sticky", &sticky).unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, board.to_bytes().unwrap()).unwrap();

        let out = dir.path().join("page.svg");
        let done = server
            .canvas_export(Parameters(CanvasExportArgs {
                format: Some("svg".into()),
                output_path: Some(out.display().to_string()),
                project: project_arg(&project),
            }))
            .await;
        assert_ne!(done.is_error, Some(true), "{}", said(&done));
        std::fs::read_to_string(out).unwrap()
    }

    #[tokio::test]
    async fn a_headless_export_of_text_has_glyphs() {
        let with_text = exported_sticky("note 0").await;
        let without_text = exported_sticky("").await;

        // Text laid out with no font to draw it in comes out as a path with no outline (`d=""`),
        // and a sticky that says something then exports as one that says nothing. The sticky's own
        // rectangle and the text's path are two paths either way, so counting them proves nothing.
        assert!(!with_text.contains(r#"d="""#), "{with_text}");
        let (with, without) = (with_text.len(), without_text.len());
        assert!(
            with > without,
            "{with} bytes, no more than the {without} of a sticky with no text: {with_text}"
        );
    }

    /// canvas_export reads `output_path` as every other tool that writes a file does: trimmed,
    /// with `~` for the home folder. It used to write the path as given, so `~/Desktop/board.svg`
    /// made a folder named `~` in the folder the agent runs in, and a stray space at the end
    /// became part of the file's name.
    #[tokio::test]
    async fn an_export_path_is_read_as_every_tool_reads_one() {
        let named = |path: &str| crate::output_path("canvas_export", Some(path)).unwrap();
        // `~`, without writing to the home folder: the path that is written is this one.
        if let Some(home) = std::env::var_os("HOME") {
            let home = Path::new(&home);
            let desktop = home.join("Desktop").join("board.svg");
            assert_eq!(named("~/Desktop/board.svg"), Some(desktop));
            assert_eq!(named(" ~/board.png \n"), Some(home.join("board.png")));
        }
        assert_eq!(named(" /a/board.svg "), Some(PathBuf::from("/a/board.svg")));

        let dir = tempfile::tempdir().unwrap();
        let server = native(dir.path());
        let project = dir.path().join("project");
        write_board(&board_file(&project), 1);
        let out = dir.path().join("page.svg");
        let done = server
            .canvas_export(Parameters(CanvasExportArgs {
                format: Some("svg".into()),
                output_path: Some(format!("{}  ", out.display())),
                project: project_arg(&project),
            }))
            .await;
        assert_ne!(done.is_error, Some(true), "{}", said(&done));
        let written = std::fs::read(&out).unwrap();
        assert_eq!(
            said(&done),
            format!(
                "Canvas exported to {} ({} bytes)",
                out.display(),
                written.len()
            )
        );
    }
}
