//! Where a canvas op runs: on the canvas of the running app, or with the app closed on the stored
//! canvas file; and what the tools that read the canvas answer.

use std::path::{Path, PathBuf};

use rmcp::model::CallToolResult;
use sawit_board::store;
use sawit_core::app_control::{self, Command};
use sawit_core::update::Version;
use serde_json::{Value, json};

use super::files::{canvas_file, with_files_resolved};
use super::stored::{CANVAS_LOCK, headless};
use super::{CanvasCommandArgs, CanvasMermaidArgs, CanvasTargetArgs};
use crate::app::pretty;
use crate::{Sawit, blocking, fail, ok};

/// The first version of the app that runs a canvas op on the canvas file it names
/// ([`Command::Canvas`]'s `file`). An older one ignores the field, which it doesn't know, and runs
/// the op on the project's canvas: a clear or a deletion meant for a design file would take layers
/// off the project's board, whose ids the design file's can match ([`app_reads_canvas_files`]).
pub(super) const APP_WITH_CANVAS_FILES: Version = Version(0, 0, 8);

impl Sawit {
    /// Runs canvas op `op` on the canvas of `project`, or on the canvas file that `file` names
    /// ([`canvas_file`]), and answers it. A running app answers every canvas tool, as it always
    /// has, on that canvas's tab. With no app running, the op runs on the stored canvas file itself
    /// ([`headless`]), which is how an agent edits a canvas while Sawit is closed. The two are
    /// never both at work, so the app's autosave and the file never race.
    pub(super) async fn canvas(
        &self,
        project: Option<String>,
        file: Option<String>,
        op: Value,
    ) -> Result<Value, String> {
        let file = canvas_file(file.as_deref())?;
        if !app_control::is_running(&self.paths) {
            let named = match file {
                Some(file) => file,
                None => self.canvas_board_file(project.as_deref())?,
            };
            let lock = self.paths.root.join(CANVAS_LOCK);
            return blocking(move || headless(&named, &lock, &op)).await?;
        }
        if let Some(file) = &file {
            self.refuse_an_app_without_canvas_files(file).await?;
        }
        op_reply(self.app_call(project, Command::Canvas { op, file }).await?)
    }

    /// Refuses an op on the canvas file `file`, before it is sent, when the running app is older
    /// than one that runs it there ([`app_reads_canvas_files`]): it would run the op on the
    /// project's canvas. The app's version is the one app_state answers with. An app updated on
    /// disk runs its old version until it restarts, while an agent started since runs the new
    /// sawit-mcp.
    async fn refuse_an_app_without_canvas_files(&self, file: &Path) -> Result<(), String> {
        let state = self.app_call(None, Command::State).await?;
        let version = state["version"].as_str().unwrap_or("an unknown version");
        if app_reads_canvas_files(version) {
            return Ok(());
        }
        Err(format!(
            "Nothing was changed: the Sawit app that is running ({version}) is older than this \
             sawit-mcp ({}), and would run the op on the project's canvas instead of {}. Quit \
             Sawit and open it again, so that it runs the version installed, then call again.",
            env!("CARGO_PKG_VERSION"),
            file.display()
        ))
    }

    /// The file `project`'s canvas is saved in while the app is closed: its
    /// `.nyawit/canvas.sawit` (the project is the one that holds the folder the agent runs in,
    /// [`crate::agent_project`], unless it names one), else `canvas.sawit` in Sawit's own
    /// folder ([`store::board_file`]). This is the app's choice of file too.
    ///
    /// A project the agent names must be a folder that is there, as the app answers only for a
    /// project one of its windows has open. A name that is no folder (a project's name rather than
    /// its path, say) would otherwise get a canvas of its own, in a new folder of that name, where
    /// neither the agent nor the user would look.
    fn canvas_board_file(&self, project: Option<&str>) -> Result<PathBuf, String> {
        let named = project.is_some_and(|project| !project.trim().is_empty());
        let dir = crate::agent_project(project);
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

    /// `canvas_add_diagram`, and `canvas_add_mermaid`, its old name: the canvas's `mermaid` op,
    /// which every version of the app runs.
    pub async fn canvas_add_diagram_tool(&self, a: CanvasMermaidArgs) -> CallToolResult {
        let op = json!({ "op": "mermaid", "source": a.source, "x": a.x, "y": a.y });
        match self.canvas(a.project, a.file, op).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    pub async fn canvas_get_tool(&self, a: CanvasTargetArgs) -> CallToolResult {
        let reply = if app_control::is_running(&self.paths) {
            self.canvas(a.project, a.file, json!({ "op": "get" })).await
        } else {
            self.canvas_get_stored(a.project, a.file).await
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
        match self.canvas(a.project, a.file, op).await {
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
    /// error is the answer, because the old canvas is months behind it. A canvas file the agent
    /// names (`file`) has no such exception: it is read as every canvas op reads it.
    async fn canvas_get_stored(
        &self,
        project: Option<String>,
        file: Option<String>,
    ) -> Result<Value, String> {
        let get = json!({ "op": "get" });
        if canvas_file(file.as_deref())?.is_some() {
            return self.canvas(project, file, get).await;
        }
        let board = self.canvas_board_file(project.as_deref())?;
        match self.canvas(project.clone(), None, get).await {
            Err(_) if !board.exists() => self.canvas_file_summary(project.as_deref()),
            reply => reply,
        }
    }

    /// The old canvas as saved on disk, listed as `canvas_get` listed it before the native canvas:
    /// the answer of [`Self::canvas_get_stored`] for an old canvas the importer can't read. It is
    /// the project's `.nyawit/canvas.excalidraw` ([`sawit_core::nyawit::legacy_canvas_file`]),
    /// else the app-wide one.
    fn canvas_file_summary(&self, project: Option<&str>) -> Result<Value, String> {
        let file = match crate::agent_project(project) {
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

/// Whether the app of `version` (as app_state gives it) runs a canvas op on the canvas file it
/// names: one of [`APP_WITH_CANVAS_FILES`] or later. So is one of this sawit-mcp's own version,
/// while that is older: a build from before the release that brings `file` has it too.
pub(super) fn app_reads_canvas_files(version: &str) -> bool {
    let since = APP_WITH_CANVAS_FILES.min(Version::current());
    Version::parse(version).is_some_and(|app| app >= since)
}

/// The fields `canvas_get` shows for one saved element.
pub(super) fn summarize_element(e: &Value) -> Value {
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
pub(super) fn op_reply(reply: Value) -> Result<Value, String> {
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
pub(super) fn command_list(reply: &Value, filter: Option<&str>) -> String {
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
