//! Tools that drive the running Sawit app over its socket (see
//! `sawit_core::app_control`): windows and tabs, panels, terminals, the Run
//! panel, SSH sessions, database tabs and the canvas.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::{self, Command, Page, Panel, Request};
use serde_json::{Value, json};

use crate::{Sawit, fail, ok};

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
    #[schemars(description = "canvas, mcp, changelog, git_graph, github or instance")]
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
    #[schemars(description = "Lines of scrollback + screen to return (default 200, max 5000)")]
    pub lines: Option<usize>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TerminalInputArgs {
    #[schemars(description = "Terminal tab index; default the active one")]
    pub tab: Option<usize>,
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
    #[schemars(description = "SQL to put in the editor (not run)")]
    pub sql: Option<String>,
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
pub struct CanvasExportArgs {
    #[schemars(description = "svg (default) or png")]
    pub format: Option<String>,
    #[schemars(description = "Where to write the file; default Sawit's cache folder")]
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
const PAGES: &str = "canvas, mcp, changelog, git_graph, github, instance";

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn project_path(p: Option<String>) -> Option<PathBuf> {
    p.filter(|p| !p.trim().is_empty()).map(PathBuf::from)
}

impl Sawit {
    /// Sends `command` to the running app; the reply's JSON or its error.
    async fn app_call(&self, project: Option<String>, command: Command) -> Result<Value, String> {
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

    async fn canvas(&self, project: Option<String>, op: Value) -> Result<Value, String> {
        self.app_call(project, Command::Canvas { op }).await
    }

    /// The board as saved on disk, for `canvas_get` while the app is closed.
    fn canvas_file_summary(&self) -> Result<Value, String> {
        let file = self.paths.root.join("canvas.excalidraw");
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

/// Where an export goes when no path is given.
fn export_path(cache: &Path, ext: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    cache.join("canvas").join(format!("canvas-{stamp}.{ext}"))
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
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
        description = "What the Sawit app shows: each window's project, sidebar panel, layout, open tabs (index, title, kind, path, unsaved) and terminal tabs (index, title, running job, script, ssh)"
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
        description = "Open a page as a tab: canvas (Excalidraw board), mcp (install page), changelog, git_graph, github, instance (selected PostgreSQL instance)"
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
            text: a.text,
            key: a.key,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(description = "Close a terminal tab")]
    async fn terminal_close(&self, Parameters(a): Parameters<TerminalCloseArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::TerminalClose { tab: a.tab })
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

    #[tool(description = "Open a SQL editor tab for a connection, optionally prefilled with SQL")]
    async fn app_open_sql(&self, Parameters(a): Parameters<OpenSqlArgs>) -> CallToolResult {
        let command = Command::OpenSql {
            connection: a.connection,
            sql: a.sql,
        };
        self.app_tool(a.project, command).await
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
        if !app_control::is_running(&self.paths) {
            return match self.canvas_file_summary() {
                Ok(v) => ok(pretty(&v)),
                Err(e) => fail(e),
            };
        }
        match self.canvas(a.project, json!({ "op": "get" })).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
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
        description = "Convert a Mermaid diagram into editable Excalidraw shapes on Sawit's canvas. (For an SVG image of a Mermaid diagram use render_mermaid)"
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
        let scene = match (a.scene, a.path) {
            (Some(scene), _) => scene,
            (None, Some(path)) => {
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => return fail(format!("Unable to read {path}: {e}")),
                };
                match serde_json::from_str(&text) {
                    Ok(scene) => scene,
                    Err(e) => return fail(format!("{path} isn't valid JSON: {e}")),
                }
            }
            (None, None) => return fail("Give a path or a scene."),
        };
        match self
            .canvas(a.project, json!({ "op": "replace", "scene": scene }))
            .await
        {
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
            export_path(&self.paths.cache_dir(), if png { "png" } else { "svg" })
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
    use super::*;

    #[test]
    fn panel_and_page_names() {
        assert_eq!(
            variant::<Panel>("Shell_Config", PANELS),
            Ok(Panel::ShellConfig)
        );
        assert_eq!(variant::<Page>("git_graph", PAGES), Ok(Page::GitGraph));
        assert!(
            variant::<Panel>("nope", PANELS)
                .unwrap_err()
                .contains("ports")
        );
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
}
