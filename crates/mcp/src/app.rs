//! Tools that drive the running Sawit app over its socket (see
//! `sawit_core::app_control`): windows and tabs, panels, terminals, the Run
//! panel, SSH sessions, database tabs and the canvas.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_board::store::{self, Stamp, Writer};
use sawit_board::{BoardDoc, ops};
use sawit_core::app_control::{self, Command, InstanceAction, Page, Panel, Request};
use serde_json::{Value, json};

use crate::{Sawit, blocking, fail, ok};

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
    #[schemars(description = "Absolute folder or file to open once Sawit runs")]
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
        description = "instances, files, git, database, run, ports, network, prompts, ssh or shell_config (Config Files)"
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
    #[schemars(description = "Start a new shell in this folder (default: the project)")]
    pub cwd: Option<String>,
    #[schemars(description = "Use a new terminal tab instead of the active idle shell")]
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
        description = "A .figcraft file to load, or an .excalidraw file to import (replaces the board)"
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
        description = "Where to write the file; default the project's .nyawit/exports folder (Sawit's cache folder outside a project)"
    )]
    pub output_path: Option<String>,
    pub project: Option<String>,
}

/// A snake_case name as one of `T`'s variants (`ports` as `Panel::Ports`).
fn variant<T: serde::de::DeserializeOwned>(name: &str, choices: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(name.trim().to_lowercase()))
        .map_err(|_| format!("Unknown “{name}”: use one of {choices}."))
}

const PANELS: &str =
    "instances, files, git, database, run, ports, network, prompts, ssh, shell_config";
const PAGES: &str = "canvas, browser, mcp, tools, changelog, git_graph, github, instance, keymap";
const INSTANCE_ACTIONS: &str = "select, start, stop, restart";

pub(crate) fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn project_path(p: Option<String>) -> Option<PathBuf> {
    p.filter(|p| !p.trim().is_empty()).map(PathBuf::from)
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
            return blocking(move || headless(&file, &op)).await?;
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
        let op = json!({ "op": "command", "command": a.command, "params": a.params });
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
/// the work of the earlier. This keeps one process's calls apart; another process's (a second
/// agent's `sawit-mcp`) are kept from it by the stamp each save checks ([`headless`]).
static STORED_CANVAS: Mutex<()> = Mutex::new(());

fn take_turn() -> MutexGuard<'static, ()> {
    // A call that panicked has left the file whole (it is written by rename), so the next may go on.
    STORED_CANVAS.lock().unwrap_or_else(PoisonError::into_inner)
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
fn headless(file: &Path, op: &Value) -> Result<Value, String> {
    let _turn = take_turn();
    // The fonts the app's text has: Sawit's own (Inter), then the system's, which no app is
    // scanning here, so they are read the first time text is laid out.
    sawit_board::fonts::register();
    sawit_board::scan_system_fonts_on_demand();
    // Stamped before it is read, as the app stamps a canvas file it opens: a save made in between
    // by another process makes this call's save refuse, rather than drop that save's edit.
    let read = Stamp::of(file);
    let mut board = store::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    run_and_save(&mut board, file, read, op)
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
/// A `replace` of a canvas with layers keeps the file it replaces as `<file>.bak` first. The app
/// can undo a replace; here every call opens the canvas afresh, so that copy is all that is left
/// of the board.
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
    if op["op"] == "replace" && !store::is_empty(&before) {
        keep_replaced_canvas(file)?;
    }
    let writer = Writer::spawn();
    writer.guard(file.to_path_buf(), read);
    writer.save(file.to_path_buf(), bytes, store::is_empty(&after));
    writer.flush();
    match writer.last_error() {
        Some(error) => Err(error),
        None => Ok(reply),
    }
}

/// Copies the canvas file at `file`, which a `replace` is about to save over, to `<file>.bak`. With
/// no file there (an old canvas imported, which stays as it is) there is nothing to keep. When the
/// copy can't be made the error says why, and the replace must not be saved.
fn keep_replaced_canvas(file: &Path) -> Result<(), String> {
    let mut bak = file.as_os_str().to_owned();
    bak.push(".bak");
    let bak = PathBuf::from(bak);
    match std::fs::copy(file, &bak) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !file.exists() => Ok(()),
        Err(e) => Err(format!(
            "The canvas wasn't replaced: it couldn't be copied to {} first: {e}",
            bak.display()
        )),
    }
}

/// Whether `path` names a `.figcraft` file.
fn is_figcraft(path: &str) -> bool {
    let extension = Path::new(path.trim()).extension();
    extension.is_some_and(|e| e.eq_ignore_ascii_case("figcraft"))
}

/// `path` as the app, whose working folder is not the agent's, can open it: `~` expanded, and a
/// relative path taken from the folder the agent runs in. It is text for a JSON op, so a folder
/// that isn't valid UTF-8 comes out with replacement characters, and the canvas says it can't be read.
fn absolute_path(path: &str) -> String {
    let path = crate::expand_home(path.trim());
    let path = match std::env::current_dir() {
        Ok(cwd) if path.is_relative() => cwd.join(path),
        _ => path,
    };
    path.to_string_lossy().into_owned()
}

/// The file `output_path` names for canvas_export, read as every tool that writes a file reads
/// one: trimmed, with `~` expanded. The canvas-file guard checks this path, and it is the one
/// written.
fn named_export(output_path: &str) -> PathBuf {
    crate::expand_home(output_path.trim())
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
        description = "Start the Sawit app if it isn't running (or bring it forward), optionally opening a folder or file. The app_* and terminal_* tools need it running; the canvas_* tools also work on the saved canvas file without it"
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
        let command = Command::TerminalRun {
            command: a.command,
            cwd: a.cwd.map(PathBuf::from),
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
        description = "Clear the whole canvas. In the app the user can undo it; with the app closed the cleared board is kept as canvas.figcraft.bak beside the canvas file"
    )]
    async fn canvas_clear(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        match self.canvas(a.project, json!({ "op": "clear" })).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Replace the canvas with a .figcraft file, or import an .excalidraw file or scene JSON in its place"
    )]
    async fn canvas_load(&self, Parameters(a): Parameters<CanvasLoadArgs>) -> CallToolResult {
        let op = match (a.scene, a.path) {
            (Some(scene), _) => json!({ "op": "replace", "scene": scene }),
            // A .figcraft file is loaded by the canvas itself, from where it is.
            (None, Some(path)) if is_figcraft(&path) => {
                json!({ "op": "replace", "path": absolute_path(&path) })
            }
            (None, Some(path)) => {
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => return fail(format!("Unable to read {path}: {e}")),
                };
                match serde_json::from_str::<Value>(&text) {
                    Ok(scene) => json!({ "op": "replace", "scene": scene }),
                    Err(e) => return fail(format!("{path} isn't valid JSON: {e}")),
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
        let png = match a.format.as_deref().unwrap_or("svg") {
            "svg" => false,
            "png" => true,
            other => return fail(format!("Unknown format “{other}”: use svg or png.")),
        };
        let named = a.output_path.as_deref().map(named_export);
        // Nothing else holds the board when the app is closed: an export must never replace it.
        if let Some(path) = &named
            && let Err(e) = crate::refuse_canvas_file("canvas_export", path)
        {
            return fail(e);
        }
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
        description = "Run one command of the canvas engine by its id, with its parameters as JSON: board.sticky {x, y, text}, node.set, align.left, edit.undo, view.zoomFit, ... The reply is the command's result, and a command the engine refuses is an error. canvas_commands lists the ids, and with a filter what each takes"
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
    if let Some(path) = path {
        cmd.arg(path);
    }
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
    Ok(match (running, path) {
        (_, Some(path)) => format!("Sawit is running with {path} open."),
        _ => "Sawit is running.".into(),
    })
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
        // `~`, without writing to the home folder: the path that is written is this one.
        if let Some(home) = std::env::var_os("HOME") {
            let home = Path::new(&home);
            let named = named_export("~/Desktop/board.svg");
            assert_eq!(named, home.join("Desktop").join("board.svg"));
            assert_eq!(named_export(" ~/board.png \n"), home.join("board.png"));
        }
        assert_eq!(named_export(" /a/board.svg "), Path::new("/a/board.svg"));

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
