//! Tools that drive the running Sawit app over its socket (see
//! `sawit_core::app_control`): windows and tabs, panels, terminals, the Run
//! panel, SSH sessions, database tabs and the canvas.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_board::store::{self, Writer};
use sawit_board::{BoardDoc, ops};
use sawit_core::app_control::{self, Command, InstanceAction, Page, Panel, Request};
use serde_json::{Value, json};

use crate::{Sawit, blocking, fail, ok};

/// Most commands answer at once; canvas ops wait for the page (the app
/// gives up after 30 s).
const REPLY_TIMEOUT: Duration = Duration::from_secs(40);
/// How long `launch_app` waits for the app to listen.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
/// `terminal_run` waits at most this long for output.
const MAX_WAIT: Duration = Duration::from_secs(120);

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WindowArgs {
    #[schemars(
        description = "Project folder (or a path inside it) whose window to use; default the frontmost window"
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
        description = "instances, files, git, database, run, ports, network, prompts, ssh or shell_config"
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
        description = "canvas, browser, mcp, tools, changelog, git_graph, github or instance"
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
    #[schemars(description = "Terminal tab index (from app_state); default the active one")]
    pub tab: Option<usize>,
    #[schemars(description = "Pane within the tab (from app_state); default: the focused pane")]
    pub pane: Option<usize>,
    #[schemars(description = "Lines of scrollback + screen to return (default 200, max 5000)")]
    pub lines: Option<usize>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalInputArgs {
    #[schemars(description = "Terminal tab index; default the active one")]
    pub tab: Option<usize>,
    #[schemars(description = "Pane within the tab (from app_state); default: the focused pane")]
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
    #[schemars(description = "Terminal tab index (from app_state)")]
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
        description = "Excalidraw element skeletons, e.g. {\"type\":\"rectangle\",\"id\":\"a\",\"x\":0,\"y\":0,\"width\":160,\"height\":60,\"label\":{\"text\":\"API\"}}, {\"type\":\"arrow\",\"x\":160,\"y\":30,\"start\":{\"id\":\"a\"},\"end\":{\"id\":\"b\"}}, {\"type\":\"text\",\"x\":0,\"y\":-40,\"text\":\"Title\"}. Types: rectangle, ellipse, diamond, text, arrow, line, frame"
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
    #[schemars(description = "An .excalidraw file to load (replaces the board)")]
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
const PAGES: &str = "canvas, browser, mcp, tools, changelog, git_graph, github, instance";
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
        let request = Request {
            project: project_path(project),
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
    /// has. With the native canvas on and no app running, the op runs on the stored canvas file
    /// itself ([`headless`]), which is how an agent edits a canvas while Sawit is closed. The two
    /// are never both at work, so the app's autosave and the file never race.
    async fn canvas(&self, project: Option<String>, op: Value) -> Result<Value, String> {
        if self.native_canvas && !app_control::is_running(&self.paths) {
            let file = self.canvas_board_file(project.as_deref());
            return blocking(move || headless(&file, &op)).await?;
        }
        op_reply(self.app_call(project, Command::Canvas { op }).await?)
    }

    /// The file `project`'s canvas is saved in: its `.nyawit/canvas.figcraft` (the project is the
    /// folder the agent runs in, unless it names one), else `canvas.figcraft` in Sawit's own
    /// folder. This is the app's choice of file too.
    fn canvas_board_file(&self, project: Option<&str>) -> PathBuf {
        store::board_file(crate::project_dir(project).as_deref(), &self.paths.root)
    }

    pub async fn canvas_get_tool(&self, a: WindowArgs) -> CallToolResult {
        let reply = if app_control::is_running(&self.paths) {
            self.canvas(a.project, json!({ "op": "get" })).await
        } else if self.native_canvas {
            self.canvas_get_stored(a.project).await
        } else {
            self.canvas_file_summary(a.project.as_deref())
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

    /// `canvas_get` with the native canvas on and the app closed: the layers of the stored canvas,
    /// as the app would show them. A project that has only its old Excalidraw canvas is read
    /// through the importer, and that file stays as it is.
    ///
    /// The one exception is an old canvas the importer can't read, while no canvas file has been
    /// made from it: the old reader says what it always did. Once the canvas file is there its own
    /// error is the answer, because the old canvas is months behind it.
    async fn canvas_get_stored(&self, project: Option<String>) -> Result<Value, String> {
        let file = self.canvas_board_file(project.as_deref());
        match self.canvas(project.clone(), json!({ "op": "get" })).await {
            Err(_) if !file.exists() => self.canvas_file_summary(project.as_deref()),
            reply => reply,
        }
    }

    /// The board as saved on disk, for `canvas_get` while the app is
    /// closed: the project's `.nyawit/canvas.excalidraw`, else the app-wide one.
    fn canvas_file_summary(&self, project: Option<&str>) -> Result<Value, String> {
        let file = match crate::project_dir(project) {
            Some(project) => sawit_core::nyawit::canvas_file(&project),
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
/// the work of the earlier.
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
/// - A change is saved through the store's [`Writer`], like the app's autosave: a temporary file
///   renamed into place, a `.bak` kept before a save that empties a canvas, and a `.nyawit/` made
///   with its `.gitignore`.
fn headless(file: &Path, op: &Value) -> Result<Value, String> {
    let _turn = take_turn();
    let mut board = store::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    run_and_save(&mut board, file, op)
}

/// Runs `op` on `board` and saves it to `file` if the op changed the document: a new document, as
/// the engine makes one for every edit, is how the app tells too. Selection and view changes are
/// not in it, so they save nothing.
fn run_and_save(board: &mut BoardDoc, file: &Path, op: &Value) -> Result<Value, String> {
    let before = board.doc().map_err(|e| e.to_string())?;
    let reply = op_reply(ops::run(board, op))?;
    let after = board.doc().map_err(|e| e.to_string())?;
    if !Arc::ptr_eq(&before, &after) {
        let bytes = board.to_bytes().map_err(|e| e.to_string())?;
        let writer = Writer::spawn();
        writer.save(file.to_path_buf(), bytes, store::is_empty(&after));
        writer.flush();
        if let Some(error) = writer.last_error() {
            return Err(error);
        }
    }
    Ok(reply)
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

pub(crate) fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (i, &c) in ALPHABET.iter().enumerate() {
        lookup[c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
    {
        let v = lookup[c as usize];
        if v == 255 {
            return Err("The canvas sent invalid base64.".into());
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

#[tool_router(router = app_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Start the Sawit app if it isn't running (or bring it forward), optionally opening a folder or file. The app_*, terminal_* and canvas_* tools need it running"
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
        description = "Open a page as a tab: canvas (Excalidraw board), browser (web browser agents can drive), mcp (install page), tools (dev tools installer), changelog, git_graph, github, instance (selected PostgreSQL instance)"
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
        description = "Run a command in Sawit's terminal (visible to the user), wait, and return the terminal's latest output. Use terminal_read to check long-running commands later, terminal_input with key ctrl-c to stop them"
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
        let tab = started["terminal"].as_u64().map(|t| t as usize);
        let wait = Duration::from_secs_f64(a.wait_secs.unwrap_or(2.0).max(0.0)).min(MAX_WAIT);
        tokio::time::sleep(wait).await;
        let read = Command::TerminalRead {
            tab,
            pane: None,
            lines: Some(a.lines.unwrap_or(100)),
        };
        match self.app_call(a.project, read).await {
            Ok(v) => ok(format!(
                "Terminal {}:\n{}",
                tab.map(|t| t.to_string()).unwrap_or_default(),
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
        description = "Type text and/or press a key in a terminal tab, e.g. answer a prompt, or key ctrl-c to interrupt"
    )]
    async fn terminal_input(&self, Parameters(a): Parameters<TerminalInputArgs>) -> CallToolResult {
        let command = Command::TerminalInput {
            tab: a.tab,
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
        description = "Start a script or saved command from the Run panel in its own terminal tab (read its output with terminal_read)"
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
        description = "Open an interactive SSH session to a saved server or ~/.ssh/config host in Sawit's terminal (drive it with terminal_input / terminal_read)"
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
        description = "List the elements on Sawit's Excalidraw canvas: id, type, position, size, text, colors and arrow bindings. Works from the saved file when the app isn't running"
    )]
    async fn canvas_get(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        self.canvas_get_tool(a).await
    }

    #[tool(
        description = "Draw on Sawit's canvas: add Excalidraw shapes, labels, text and arrows (arrows bind to shapes by id). Opens the canvas tab"
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
        description = "Convert a Mermaid diagram into editable Excalidraw shapes on Sawit's canvas: boxes, arrows bound to them, labels. Charts (pie, gantt, xychart, quadrant, journey, sankey, architecture) are placed as one image. (For an SVG image of a Mermaid diagram use render_mermaid)"
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

    #[tool(description = "Clear the whole canvas (the user can undo in the app)")]
    async fn canvas_clear(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        match self.canvas(a.project, json!({ "op": "clear" })).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Replace the canvas with an .excalidraw file or scene JSON")]
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
        let op = json!({ "op": if png { "export_png" } else { "export_svg" } });
        let project = crate::project_dir(a.project.as_deref());
        let value = match self.canvas(a.project, op).await {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        let bytes = if png {
            match base64_decode(value["png_base64"].as_str().unwrap_or_default()) {
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
        let path = a.output_path.map(PathBuf::from).unwrap_or_else(|| {
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
            assert_eq!(
                read_or_input,
                "Pane within the tab (from app_state); default: the focused pane"
            );
        }
        let close = pane(schemars::schema_for!(TerminalCloseArgs));
        assert!(close.contains("the whole tab"), "{close}");
        assert!(!close.contains("focused pane"), "{close}");
    }

    #[test]
    fn decodes_base64() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(
            base64_decode("iVBO\nRw==").unwrap(),
            [0x89, b'P', b'N', b'G']
        );
        assert!(base64_decode("@@").is_err());
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

    /// A server with the native canvas on, as `SAWIT_NATIVE_CANVAS=1` has it, whose home is in `dir`.
    fn native(dir: &Path) -> Sawit {
        Sawit::new(AppPaths {
            root: dir.join("home"),
        })
        .with_native_canvas(true)
    }

    fn project_arg(project: &Path) -> Option<String> {
        Some(project.display().to_string())
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
        let project = dir.path().join("project");
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
        let project = dir.path().join("project");

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
        let list = |filter: Option<&str>| {
            server.canvas_commands(Parameters(CanvasCommandsArgs {
                filter: filter.map(str::to_string),
                project: project_arg(&dir.path().join("project")),
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
}
