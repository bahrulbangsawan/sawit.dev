//! Where a canvas op runs: on the canvas of the running app, or with the app closed on the stored
//! canvas file; and what the tools that read the canvas answer.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use rmcp::model::CallToolResult;
use sawit_board::store;
use sawit_core::app_control::{self, Command, Request};
use sawit_core::instances::AppPaths;
use sawit_core::update::Version;
use serde_json::{Value, json};

use super::files::{canvas_file, with_files_resolved};
use super::stored::{CANVAS_LOCK, headless};
use super::{CanvasCommandArgs, CanvasMermaidArgs, CanvasTargetArgs};
use crate::app::{REPLY_TIMEOUT, pretty};
use crate::{Sawit, blocking, fail, ok};

/// The first version of the app that runs a canvas op on the canvas file it names
/// ([`Command::Canvas`]'s `file`). An older one ignores the field, which it doesn't know, and runs
/// the op on the project's canvas: a clear or a deletion meant for a design file would take layers
/// off the project's board, whose ids the design file's can match ([`app_reads_canvas_files`]).
pub(super) const APP_WITH_CANVAS_FILES: Version = Version(0, 0, 8);

/// The version app_state answered with, by the socket of the app that answered, and that socket
/// file ([`Listener`]): [`refuse_an_app_without_canvas_files`] asks an app once, rather than
/// before every op on a canvas file, for an answer that is the app's whole state, which the app
/// gathers from every window on its UI thread. An app makes its socket anew as it starts, so one
/// opened again (restarted, or updated and opened) is asked again.
static APP_VERSIONS: Mutex<BTreeMap<PathBuf, (Listener, String)>> = Mutex::new(BTreeMap::new());

/// The socket file an app listens on, as that app's own: the file's device, inode and change
/// time, which the socket another app makes at that path doesn't share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Listener(u64, u64, i64, i64);

impl Listener {
    /// The socket file at `socket`, if there is one.
    pub(super) fn of(socket: &Path) -> Option<Self> {
        let file = std::fs::metadata(socket).ok()?;
        Some(Self(
            file.dev(),
            file.ino(),
            file.ctime(),
            file.ctime_nsec(),
        ))
    }
}

/// The version the app that listens on `listener`, at `socket`, answered app_state with, if it
/// was asked.
fn known_version(socket: &Path, listener: &Listener) -> Option<String> {
    let known = APP_VERSIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let (answered, version) = known.get(socket)?;
    (answered == listener).then(|| version.clone())
}

/// The socket file a connection reached, to keep what its app says under ([`APP_VERSIONS`]): the
/// one at the path `before` the connection was made, when it is still the one there `after`. A
/// socket file unlinked from the path never comes back to it, so the connection, made in
/// between, reached that file's app. When they differ, another app's socket took the path in
/// between (`cli::serve` unlinks the old file, then binds its own, so two apps starting at once
/// can swap it), the connection may have reached either app, and nothing is kept or used.
pub(super) fn same_listener(before: Option<Listener>, after: Option<Listener>) -> Option<Listener> {
    before.filter(|before| after.as_ref() == Some(before))
}

/// What became of a canvas op offered to the app ([`offer`]).
enum Offered {
    /// The app is running, and this is its answer.
    Answered(Result<Value, String>),
    /// No app is listening: the op and its canvas file, unsent, for the stored canvas.
    NotRunning(Value, Option<PathBuf>),
}

impl Sawit {
    /// Runs canvas op `op` on the canvas of `project`, or on the canvas file that `file` names
    /// ([`canvas_file`]), and answers it. A running app answers every canvas tool, as it always
    /// has, on that canvas's tab ([`offer`]). With no app running, the op runs on the stored
    /// canvas file itself ([`headless`]), which is how an agent edits a canvas while Sawit is
    /// closed. The two are never both at work, so the app's autosave and the file never race.
    pub(super) async fn canvas(
        &self,
        project: Option<String>,
        file: Option<String>,
        op: Value,
    ) -> Result<Value, String> {
        let file = canvas_file(file.as_deref())?;
        let (op, file) = match self.offer_to_app(project.clone(), file, op).await? {
            Offered::Answered(reply) => return op_reply(reply?),
            Offered::NotRunning(op, file) => (op, file),
        };
        let named = match file {
            Some(file) => file,
            None => self.canvas_board_file(project.as_deref())?,
        };
        let lock = self.paths.root.join(CANVAS_LOCK);
        blocking(move || headless(&named, &lock, &op)).await?
    }

    /// [`offer`]s canvas op `op` to the running app, for the canvas of `project` or of the canvas
    /// file `file`, off the async threads, once the requests it may send are made.
    async fn offer_to_app(
        &self,
        project: Option<String>,
        file: Option<PathBuf>,
        op: Value,
    ) -> Result<Offered, String> {
        let request = self.request_for(project);
        let state = file
            .is_some()
            .then(|| self.request_for(None)(Command::State));
        let paths = self.paths.clone();
        blocking(move || offer(&paths, request, state, op, file)).await
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
        let op = in_work(op, a.work);
        match self.canvas(a.project, a.file, op).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    pub async fn canvas_get_tool(&self, a: CanvasTargetArgs) -> CallToolResult {
        let got = async {
            let file = canvas_file(a.file.as_deref())?;
            let get = json!({ "op": "get" });
            match self.offer_to_app(a.project.clone(), file, get).await? {
                Offered::Answered(reply) => op_reply(reply?),
                Offered::NotRunning(..) => self.canvas_get_stored(a.project, a.file).await,
            }
        };
        match got.await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    pub async fn canvas_command_tool(&self, a: CanvasCommandArgs) -> CallToolResult {
        let params = a
            .params
            .map(|params| with_files_resolved(&a.command, params));
        let op = json!({ "op": "command", "command": a.command, "params": params });
        // The view and the page on show are the user's, with the app closed too: an agent that
        // moved them there would move them once the app runs.
        if let Some(refused) = sawit_board::ops::agent_refusal(&op) {
            return fail(refused);
        }
        let op = in_work(op, a.work);
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

/// Offers canvas op `op` to the app that listens in `paths`, for the canvas of the project the
/// requests `request` makes name, or of the canvas file `file`: the app's answer, or, with no app
/// listening, the op back, unsent. The requests are made before. The app takes one connection at
/// a time, reading it to its end before it takes the next, and drops one that sends nothing for
/// 5 s: a connection is made here right before its request goes on it, with nothing that can wait
/// on the disk or on a thread in between. The op goes on the connection that found the app
/// running. With `file`, the app's version (`state`) goes on it first, when it isn't known, and
/// the op on a second ([`refuse_an_app_without_canvas_files`]).
fn offer(
    paths: &AppPaths,
    request: impl FnOnce(Command) -> Request,
    state: Option<Request>,
    op: Value,
    file: Option<PathBuf>,
) -> Offered {
    let socket = paths.cli_socket();
    let before = file.as_ref().and_then(|_| Listener::of(&socket));
    let Some(app) = app_control::connect(paths) else {
        return Offered::NotRunning(op, file);
    };
    let mut app = Some(app);
    if let (Some(named), Some(state)) = (&file, &state) {
        let listener = same_listener(before, Listener::of(&socket));
        if let Err(refused) =
            refuse_an_app_without_canvas_files(paths, &mut app, listener, state, named)
        {
            return Offered::Answered(Err(refused));
        }
    }
    let request = request(Command::Canvas { op, file });
    Offered::Answered(match app {
        Some(app) => app_control::exchange(app, &request, REPLY_TIMEOUT),
        None => app_control::send(paths, &request, REPLY_TIMEOUT),
    })
}

/// Refuses an op on the canvas file `file`, before it is sent, when the running app is older
/// than one that runs it there ([`app_reads_canvas_files`]): it would run the op on the
/// project's canvas. The app's version is the one app_state answers with: `state`, asked on `app`,
/// a connection to the app, which that uses up. An app updated on disk runs its old version
/// until it restarts, while an agent started since runs the new sawit-mcp. Each app is asked
/// once: when the connection reached the socket file `listener` ([`same_listener`]), the version
/// that file's app gave is used, and one it gives now kept ([`APP_VERSIONS`]).
pub(super) fn refuse_an_app_without_canvas_files(
    paths: &AppPaths,
    app: &mut Option<UnixStream>,
    listener: Option<Listener>,
    state: &Request,
    file: &Path,
) -> Result<(), String> {
    let socket = paths.cli_socket();
    let known = (listener.as_ref()).and_then(|listener| known_version(&socket, listener));
    let version = match known {
        Some(version) => version,
        None => {
            let answer = match app.take() {
                Some(app) => app_control::exchange(app, state, REPLY_TIMEOUT),
                None => app_control::send(paths, state, REPLY_TIMEOUT),
            }?;
            let version = answer["version"].as_str().unwrap_or("an unknown version");
            if let Some(listener) = listener {
                let mut known = APP_VERSIONS.lock().unwrap_or_else(PoisonError::into_inner);
                known.insert(socket, (listener, version.to_string()));
            }
            version.to_string()
        }
    };
    if app_reads_canvas_files(&version) {
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

/// Whether the app of `version` (as app_state gives it) runs a canvas op on the canvas file it
/// names: one of [`APP_WITH_CANVAS_FILES`] or later. So is one of this sawit-mcp's own version,
/// while that is older: a build from before the release that brings `file` has it too.
pub(super) fn app_reads_canvas_files(version: &str) -> bool {
    let since = APP_WITH_CANVAS_FILES.min(Version::current());
    Version::parse(version).is_some_and(|app| app >= since)
}

/// `op` with the `work` it belongs to, when it names one.
pub(super) fn in_work(mut op: Value, work: Option<String>) -> Value {
    if let Some(work) = work.filter(|work| !work.trim().is_empty()) {
        op["work"] = json!(work);
    }
    op
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
