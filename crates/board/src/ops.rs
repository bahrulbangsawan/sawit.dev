//! The canvas's control ops: the `{"op": ...}` messages of the agent wire contract (spec, "Agent
//! wire contract"), run on a [`BoardDoc`] and answered as JSON.
//!
//! Every answer is `{ok: true, ...}` or `{ok: false, error}`, and an op never panics: a failure of
//! the engine, or an op that is malformed, is an `error` the agent can read. The ops are `get`,
//! `add`, `update`, `delete`, `clear`, `replace`, `command`, `commands`, `pointer`, `mermaid` and
//! `key`; the others of the contract get their own answer, "<op> is not supported yet".
//!
//! Each op that draws, changes or removes layers (all but `command`, which is the engine's) is one
//! undo step, and when the engine refuses a part of it, none of it is done. A layer is named by the
//! id an agent gave it when it drew it, or by its node id ([`skeleton::resolve`]): an agent's id
//! wins.
//!
//! The `pointer` and `key` ops take the paths of the pointer and the keyboard on the canvas
//! (`crate::input`), so what an agent does with them is what a mouse or a keyboard does there.
//! What they ask of the view is left in `session.ui_requests`, as after the same input in the
//! window, for the view to carry out.
//!
//! A canvas holds one document, and the view saves the one on show over the canvas's file, so no
//! `command` op may leave another document open or on show. The ones that do that by name are
//! refused up front ([`HANDLED_BY_SAWIT`], as the steps of a `command.batch` too); any other
//! command that does it anyway (`file.recover`, `version.duplicate`, `branch.create`...) is undone
//! and answers an error. A `command` op that fails also leaves the file's version history and
//! branches as they were ([`FileHistory`]): they are part of the file, and the engine keeps them
//! outside undo.

use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_doc::{BoardItem, Document, NodeId};
use figcraft_engine::{EngineError, PointerKind, Result as EngineResult, Session, UiRequest};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::doc::{BoardDoc, BoardError};
use crate::input::{self, KeyOutcome, Mods};
use crate::skeleton::{self, Applied, Label, Skeleton};
use crate::{excalidraw, policy, summary};
use sawit_core::mermaid::{Palette, render};

/// The commands the `command` op refuses: they switch, open, close or write documents behind the
/// canvas's store. Sawit does these itself.
///
/// This is the cheap first check, and the ids can't be a complete list: [`Documents`] catches what
/// else a command does to the session's documents.
const HANDLED_BY_SAWIT: &[&str] = &[
    "file.new",
    "file.newBoard",
    "file.newFromTemplate",
    "file.open",
    "file.openRecent",
    "file.close",
    "file.duplicate",
    "file.save",
    "file.saveAs",
    "window.new",
    "doc.nextTab",
    "doc.previousTab",
];

/// The commands that open a document in the session. The `command` op never leaves one open
/// ([`Documents`] undoes it), so none of these can succeed through it.
const OPENS_DOCUMENTS: &[&str] = &["file.recover", "version.duplicate", "branch.open"];

/// The command that runs other commands as its steps.
const BATCH: &str = "command.batch";

/// How far right of what the page holds the `mermaid` op puts a diagram that names no place.
const MERMAID_GAP: f64 = 100.0;

/// Runs `op` (an object whose `op` is the op's name) on `board` and answers it: `{ok: true, ...}`,
/// or `{ok: false, error}`.
///
/// - `get` answers `{ok, elements}`: a summary ([`summary::summarize`]) of each top-level layer of
///   the page on show, back to front.
/// - `add` (`{elements: [Skeleton]}`) draws the elements ([`skeleton::apply`]) and answers
///   `{ok, added, ids, notes?}`: the node id of each element drawn, in the order they were given,
///   and a note for each one skipped. A skipped element has no id, so `ids[i]` is `elements[i]`
///   only when nothing before it was skipped. What an element leaves out is its default: a missing
///   `x` or `y` is 0, and an arrow or a line with no points, size or end is drawn 100 long.
/// - `update` (`{elements: [{id, ...}]}`) changes the layers the ids name, and answers
///   `{ok, updated, missing, notes?}`: how many it found, and the ids it didn't. An element can
///   change:
///   - `x`, `y`, `width` and `height`: the layer's box, in page coordinates, as `get` gives it;
///   - `text` (or `label.text`): a connector's label, the note of a sticky, the text of a shape or
///     of a text layer;
///   - `name`: the layer's name, a section's title;
///   - `backgroundColor` and `strokeColor`, by the rules `add` colours by ([`skeleton::set_colors`]).
///
///   The rest of an element is ignored. So is a `text` that a layer has no place for (ink, a
///   section), with a note.
/// - `delete` (`{ids}`) answers `{ok, deleted, missing}`. A connector's label goes with it.
/// - `clear` deletes every top-level layer of the page on show, and answers `{ok}`.
/// - `replace` (`{scene}` or `{path}`) loads a canvas in place of the canvas's pages and answers
///   `{ok, elements, notes?}`: `elements` as `get` gives them, and `notes` on what the importer
///   left out. The new canvas is an Excalidraw scene (`scene`, or an `.excalidraw` file at `path`,
///   which [`excalidraw::import`] draws), or the `.figcraft` file at `path`. A `scene` wins over a
///   `path`. It runs in the canvas's own session, so the canvas stays what it is (a board, with
///   its file), and undo brings the old pages back. A scene that has no `elements`, and a file
///   that can't be read, are errors that change nothing.
/// - `command` (`{command, params?}`) runs engine command `command` and answers `{ok, result}`,
///   or the engine's own `error`. It refuses the commands Sawit handles itself, as the steps of a
///   `command.batch` too ("<id> is handled by Sawit"). A command that would leave the canvas with
///   another document open or on show is undone and answers "<id> would open or switch documents;
///   the canvas keeps its one document".
/// - `commands` answers `{ok, commands: [{id, label, params}]}`: the engine's commands that
///   `command` runs and that the canvas doesn't hide, with the parameters each documents.
/// - `pointer` (`{kind, x, y, shift?, alt?, cmd?}`) sends the tools a press (`down`), a move with
///   the button down (`move`), its release (`up`), a double-click (`double`) or a move without a
///   button (`hover`) at page point (`x`, `y`), with the modifier keys given held, and answers
///   `{ok, selection, tool}`: the ids selected then, and the tool on.
/// - `mermaid` (`{source, x?, y?}`) draws Mermaid `source` as board shapes that can be edited, in
///   one undo step, and answers `{ok, added, ids}`, with `notes` on what couldn't be drawn. The
///   diagram's top left corner goes to `x`, `y`; what they leave out is 100 px right of what the
///   page holds and level with its top, or 0 on an empty page. Every error starts "mermaid: ".
/// - `key` (`{key, shift?, alt?, cmd?, ctrl?}`) presses `key` on the canvas, with the modifier
///   keys given held, as the keyboard does ([`input::route_key`]), and answers `{ok, handled}`:
///   whether the canvas took the key. `key` is FigCraft's name for it: a letter or a digit,
///   `Escape`, `Enter`, `Backspace`, `Delete`, `ArrowLeft`... The shortcut of a command Sawit
///   carries out itself (save, export, import, copy as) is taken, though the op carries out
///   nothing for it.
pub fn run(board: &mut BoardDoc, op: &Value) -> Value {
    match op.get("op").and_then(Value::as_str) {
        Some("get") => get(board),
        Some("add") => add(board, op),
        Some("update") => update(board, op),
        Some("delete") => delete(board, op),
        Some("clear") => clear(board),
        Some("replace") => replace(board, op),
        Some("command") => command(board, op),
        Some("commands") => commands(),
        Some("pointer") => pointer(board, op),
        Some("mermaid") => mermaid(board, op),
        Some("key") => key(board, op),
        Some(other) => failure(format!("{other} is not supported yet")),
        None => failure(r#"an op needs a name, like {"op": "get"}"#),
    }
}

/// `{ok: false, error}`.
fn failure(error: impl Display) -> Value {
    json!({"ok": false, "error": error.to_string()})
}

fn get(board: &BoardDoc) -> Value {
    match summaries(board) {
        Ok(elements) => json!({"ok": true, "elements": elements}),
        Err(error) => failure(error),
    }
}

/// The summaries of the layers of the page on show.
fn summaries(board: &BoardDoc) -> Result<Vec<Value>, BoardError> {
    let ds = board.session.ds()?;
    Ok(summary::summarize(&ds.doc, ds.page))
}

/// The top-level layers of the page on show, a connector's label among them.
fn page_layers(board: &BoardDoc) -> Result<Vec<NodeId>, BoardError> {
    let ds = board.session.ds()?;
    let page = ds.doc.node(ds.page);
    Ok(page.map(|page| page.children.clone()).unwrap_or_default())
}

/// An id as text: a string that isn't empty, or a number (the engine's node ids are numbers).
fn id_text(id: &Value) -> Option<String> {
    match id {
        Value::String(id) if !id.is_empty() => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

/// The layer `id` names, for an op: an agent's id, else a node id ([`skeleton::resolve`]), if it
/// is a layer of a page. The page is no element, though its node id is a number like the others'.
fn layer_named(doc: &Document, id: &str) -> Option<NodeId> {
    let layer = skeleton::resolve(doc, id)?;
    let is_page = doc.node(layer)?.is_page();
    (!is_page && doc.page_of(layer).is_some()).then_some(layer)
}

fn add(board: &mut BoardDoc, op: &Value) -> Value {
    let items = match skeletons(op) {
        Ok(items) => items,
        Err(error) => return failure(format!("add: {error}")),
    };
    match skeleton::apply(board, &items, (0.0, 0.0)) {
        Ok(applied) => added(&applied),
        Err(error) => failure(format!("add: {error}")),
    }
}

/// The elements of an `add`, each read as a [`Skeleton`].
fn skeletons(op: &Value) -> Result<Vec<Skeleton>, String> {
    let items = op.get("elements").and_then(Value::as_array).ok_or(
        r#"it needs the elements to draw, like {"elements": [{"type": "rectangle", "x": 0, "y": 0}]}"#,
    )?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Skeleton::deserialize(item).map_err(|error| format!("elements[{index}]: {error}"))
        })
        .collect()
}

/// `{ok, added, ids}` for what `apply` made, and the `notes` on what it skipped when it skipped
/// something.
fn added(applied: &Applied) -> Value {
    let mut reply = json!({"ok": true, "added": applied.ids.len(), "ids": applied.ids});
    if !applied.notes.is_empty() {
        reply["notes"] = json!(applied.notes);
    }
    reply
}

/// One element of an `update`: the layer it names, and what it changes.
struct Change {
    id: String,
    fields: Fields,
}

/// What an `update` element can change. What it leaves out stays as it is, and any other key is
/// ignored.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Fields {
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    text: Option<String>,
    label: Option<Label>,
    name: Option<String>,
    #[serde(rename = "backgroundColor")]
    background_color: Option<String>,
    #[serde(rename = "strokeColor")]
    stroke_color: Option<String>,
}

impl Change {
    fn read(item: &Value) -> Result<Self, String> {
        let id = item
            .get("id")
            .and_then(id_text)
            .ok_or("it needs the `id` of a layer")?;
        let fields = Fields::deserialize(item).map_err(|error| error.to_string())?;
        Ok(Self { id, fields })
    }

    /// The text to set: `text`, else the `label`'s, as for a shape in a skeleton.
    fn text(&self) -> Option<&str> {
        let label = self.fields.label.as_ref().map(|label| label.text.as_str());
        self.fields.text.as_deref().or(label)
    }
}

/// The elements of an `update`.
fn changes(op: &Value) -> Result<Vec<Change>, String> {
    let items = op
        .get("elements")
        .and_then(Value::as_array)
        .ok_or(r#"it needs the changes to make, like {"elements": [{"id": "a", "x": 10}]}"#)?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Change::read(item).map_err(|error| format!("elements[{index}]: {error}"))
        })
        .collect()
}

fn update(board: &mut BoardDoc, op: &Value) -> Value {
    let changes = match changes(op) {
        Ok(changes) => changes,
        Err(error) => return failure(format!("update: {error}")),
    };
    let (mut updated, mut missing, mut notes) = (0, Vec::new(), Vec::new());
    let result = board.session.grouped("Update elements", |session| {
        for (index, change) in changes.iter().enumerate() {
            let Some(layer) = layer_named(session.doc()?, &change.id) else {
                missing.push(change.id.clone());
                continue;
            };
            change_layer(session, layer, index, change, &mut notes).map_err(|error| {
                EngineError::Other(format!("elements[{index}] ({}): {error}", change.id))
            })?;
            updated += 1;
        }
        Ok(())
    });
    match result {
        Ok(()) => {
            let mut reply = json!({"ok": true, "updated": updated, "missing": missing});
            if !notes.is_empty() {
                reply["notes"] = json!(notes);
            }
            reply
        }
        Err(error) => failure(format!("update: {error}")),
    }
}

/// The numbers of `pairs` that are given, as params.
fn numbers<const N: usize>(pairs: [(&str, Option<f64>); N]) -> Map<String, Value> {
    pairs
        .into_iter()
        .filter_map(|(key, number)| Some((key.to_string(), json!(number?))))
        .collect()
}

/// Makes `change`, element `index` of an `update`, to `layer`: its size, then its place (so that
/// it ends with the top-left it was given), its text, its name (which wins over what the text
/// would name the layer) and its colours.
///
/// A place is in page coordinates, the ones `get` shows and `add` takes, whatever the layer is
/// inside of: `node.set`'s `x` and `y` are the parent's, and a layer put into a frame by its
/// `frameId` would land elsewhere.
fn change_layer(
    session: &mut Session,
    layer: NodeId,
    index: usize,
    change: &Change,
    notes: &mut Vec<String>,
) -> EngineResult<()> {
    let fields = &change.fields;
    let size = numbers([("width", fields.width), ("height", fields.height)]);
    if !size.is_empty() {
        session.execute("node.set", &json!({"ids": [layer], "props": size}))?;
    }
    let mut at = numbers([("x", fields.x), ("y", fields.y)]);
    if !at.is_empty() {
        at.insert("ids".into(), json!([layer]));
        session.execute("transform.moveTo", &Value::Object(at))?;
    }
    if let Some(text) = change.text()
        && !set_text(session, layer, text)?
    {
        let kind = summary::element_type(session.doc()?, layer);
        notes.push(format!(
            "elements[{index}]: `text` is ignored, because a layer of type {kind} has no text"
        ));
    }
    if let Some(name) = &fields.name {
        let props = json!({"name": name});
        session.execute("node.set", &json!({"ids": [layer], "props": props}))?;
    }
    if fields.background_color.is_some() || fields.stroke_color.is_some() {
        skeleton::set_colors(
            session,
            layer,
            fields.background_color.as_deref(),
            fields.stroke_color.as_deref(),
        )?;
    }
    Ok(())
}

/// Sets the text of `layer`: the label of a connector (an empty one removes it), the note of a
/// sticky, the text of a shape or of a text layer. `false` when the layer has no text to set.
fn set_text(session: &mut Session, layer: NodeId, text: &str) -> EngineResult<bool> {
    enum Place {
        Label,
        Layer(NodeId),
        Nowhere,
    }
    let place = {
        let doc = session.doc()?;
        match doc.node(layer).and_then(|node| node.board.as_ref()) {
            Some(BoardItem::Connector(_) | BoardItem::Label { .. }) => Place::Label,
            _ => summary::text_layer(doc, layer).map_or(Place::Nowhere, Place::Layer),
        }
    };
    match place {
        Place::Label => {
            session.execute("board.label", &json!({"id": layer, "text": text}))?;
        }
        Place::Layer(text_layer) => {
            let props = json!({"characters": text});
            session.execute("node.set", &json!({"ids": [text_layer], "props": props}))?;
        }
        Place::Nowhere => return Ok(false),
    }
    Ok(true)
}

fn delete(board: &mut BoardDoc, op: &Value) -> Value {
    let ids = match id_list(op) {
        Ok(ids) => ids,
        Err(error) => return failure(format!("delete: {error}")),
    };
    let doc = match board.doc() {
        Ok(doc) => doc,
        Err(error) => return failure(format!("delete: {error}")),
    };
    let (mut found, mut missing) = (Vec::new(), Vec::new());
    for id in ids {
        match layer_named(&doc, &id) {
            Some(layer) if !found.contains(&layer) => found.push(layer),
            Some(_) => {}
            None => missing.push(id),
        }
    }
    // Without layers the engine's delete takes the selection, so it isn't asked.
    if !found.is_empty() {
        let selection = board.session.selection();
        if let Err(error) = board.execute("edit.delete", &json!({"ids": found})) {
            return failure(format!("delete: {error}"));
        }
        // The selection is the user's: what the deletion left of it stays selected.
        if !selection.is_empty() {
            board.session.set_selection(selection);
        }
    }
    json!({"ok": true, "deleted": found.len(), "missing": missing})
}

/// The `ids` of a `delete`, each an id as [`id_text`] reads it.
fn id_list(op: &Value) -> Result<Vec<String>, String> {
    let ids = op
        .get("ids")
        .and_then(Value::as_array)
        .ok_or(r#"it needs the ids to delete, like {"ids": ["a", "b"]}"#)?;
    ids.iter()
        .enumerate()
        .map(|(index, id)| id_text(id).ok_or_else(|| format!("ids[{index}] is not an id")))
        .collect()
}

fn clear(board: &mut BoardDoc) -> Value {
    let layers = match page_layers(board) {
        Ok(layers) => layers,
        Err(error) => return failure(format!("clear: {error}")),
    };
    // Without layers the engine's delete takes the selection, so it isn't asked.
    if layers.is_empty() {
        return json!({"ok": true});
    }
    let cleared = board.session.grouped("Clear canvas", |session| {
        session.execute("edit.delete", &json!({"ids": layers}))
    });
    match cleared {
        Ok(_) => json!({"ok": true}),
        Err(error) => failure(format!("clear: {error}")),
    }
}

fn replace(board: &mut BoardDoc, op: &Value) -> Value {
    let incoming = match incoming(op) {
        Ok(incoming) => incoming,
        Err(error) => return failure(error),
    };
    if let Err(error) = replace_pages(board, &incoming.import) {
        return failure(format!("replace: {error}"));
    }
    let mut reply = get(board);
    if !incoming.notes.is_empty() {
        reply["notes"] = json!(incoming.notes);
    }
    reply
}

/// What a `replace` brings in.
struct Incoming {
    /// The params of the `file.import` that loads it.
    import: Value,
    /// What the Excalidraw importer left out of a scene.
    notes: Vec<String>,
}

/// What the `replace` op `op` brings in: the scene it carries, else the file its `path` names
/// (read here when it is an Excalidraw one). Anything that can't be taken is the reply's error,
/// before the canvas is touched.
fn incoming(op: &Value) -> Result<Incoming, String> {
    if let Some(scene) = op.get("scene").filter(|scene| !scene.is_null()) {
        return from_scene(scene).map_err(|error| format!("replace: {error}"));
    }
    let Some(path) = op.get("path").and_then(Value::as_str) else {
        return Err(
            r#"replace needs the scene or the file to load, like {"op": "replace", "path": "/path/to/board.figcraft"}"#
                .into(),
        );
    };
    let extension = Path::new(path).extension().and_then(OsStr::to_str);
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("figcraft") => Ok(Incoming {
            import: json!({"path": path}),
            notes: Vec::new(),
        }),
        Some("excalidraw") => {
            let scene = excalidraw::read_file(Path::new(path))
                .map_err(|error| format!("replace: {error}"))?;
            from_scene(&scene).map_err(|error| format!("replace: {path}: {error}"))
        }
        _ => Err(format!(
            "replace: {path} is not a .figcraft or .excalidraw file"
        )),
    }
}

/// An Excalidraw `scene` as something the engine imports: it is drawn on a board of its own, and
/// that board is the `.figcraft` that comes in. So a scene replaces the canvas the way a file
/// does, as pages, and what it is drawn on is as the importer makes it (a light page, which the
/// scene's own colours are made for).
fn from_scene(scene: &Value) -> Result<Incoming, BoardError> {
    let (drawn, notes) = excalidraw::to_board("Canvas", scene)?;
    let file = drawn.to_bytes()?;
    Ok(Incoming {
        import: json!({"data": STANDARD.encode(file), "name": "scene.figcraft"}),
        notes,
    })
}

/// Replaces the pages of the canvas with those that the `file.import` of `import` brings in (a
/// `.figcraft` file or its bytes), in the canvas's own session as one undo step: they are added to
/// the document, then the old ones are deleted. An import that fails, a file that can't be read
/// say, fails the step, and the canvas is as it was.
///
/// The canvas keeps its document, so it stays a board (a design file's frames come in as layers of
/// one), keeps its file and what is saved of it, and undo brings the old pages back.
fn replace_pages(board: &mut BoardDoc, import: &Value) -> Result<(), BoardError> {
    board.session.grouped("Replace canvas", |session| {
        let old = session.doc()?.pages.clone();
        session.execute("file.import", import)?;
        for page in old {
            session.execute("page.delete", &json!({"id": page}))?;
        }
        Ok(())
    })?;
    Ok(())
}

fn commands() -> Value {
    let commands: Vec<Value> = figcraft_engine::cmd::all_commands()
        .iter()
        .filter(|command| listed(command.id))
        .map(|command| json!({"id": command.id, "label": command.label, "params": command.params}))
        .collect();
    json!({"ok": true, "commands": commands})
}

/// Whether the `commands` op offers command `id`: the canvas doesn't hide it, and the `command`
/// op runs it to a result. That one rule leaves out what Sawit does itself ([`HANDLED_BY_SAWIT`],
/// the save and the like) and what only opens a document ([`OPENS_DOCUMENTS`]), so every command
/// listed is one an agent can call.
fn listed(id: &str) -> bool {
    !policy::is_hidden(id) && handled(id).is_none() && !OPENS_DOCUMENTS.contains(&id)
}

/// The command in [`HANDLED_BY_SAWIT`] that `id` is, if it is one.
fn handled(id: &str) -> Option<&'static str> {
    HANDLED_BY_SAWIT
        .iter()
        .find(|handled| **handled == id)
        .copied()
}

fn command(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(id) = op.get("command").and_then(Value::as_str) else {
        return failure(
            r#"a command op needs a command id, like {"op": "command", "command": "view.zoomIn"}"#,
        );
    };
    let params = match op.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(params) => params.clone(),
    };
    if let Some(refused) = refused(id, &params) {
        return failure(format!("{refused} is handled by Sawit"));
    }
    let history = FileHistory::of(&board.session);
    let ran = if id == BATCH {
        batch(board, &params)
    } else {
        single(board, id, &params)
    };
    match ran {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => {
            if let Some(history) = history {
                history.restore(&mut board.session);
            }
            failure(error)
        }
    }
}

/// The version history and branches of the canvas's file, as a command op found them.
///
/// The engine keeps them outside undo, so neither its rollback of a failed command or group nor
/// [`Documents::restore`] puts them back: a refused `branch.create` has already added its branch,
/// with two copies of the document, before the guard closes the branch's tab. A command op that
/// fails puts them back, so the canvas's next save writes nothing of what the op tried.
struct FileHistory {
    /// The uid of the canvas's document, which holds them.
    uid: u64,
    versions: Vec<figcraft_format::Version>,
    branches: Vec<figcraft_format::Branch>,
}

impl FileHistory {
    /// Those of the document on show, the canvas's. `None` without one.
    fn of(session: &Session) -> Option<Self> {
        let ds = session.ds().ok()?;
        Some(Self {
            uid: ds.uid,
            versions: ds.versions.clone(),
            branches: ds.branches.clone(),
        })
    }

    /// Puts them back on the document they were read from.
    fn restore(self, session: &mut Session) {
        if let Some(ds) = session.docs.iter_mut().find(|ds| ds.uid == self.uid) {
            ds.versions = self.versions;
            ds.branches = self.branches;
        }
    }
}

/// Why a command op has no result.
#[derive(Debug, Error)]
enum Failed {
    /// The engine refused the command, or it failed.
    #[error(transparent)]
    Engine(#[from] BoardError),
    /// The command would have left the canvas with another document open or on show; the canvas is
    /// as it was.
    #[error("{0} would open or switch documents; the canvas keeps its one document")]
    Switched(String),
}

/// The command in [`HANDLED_BY_SAWIT`] that running `id` with `params` would reach, if any: `id`
/// itself, or a step of a `command.batch`, however deeply batches are nested.
fn refused(id: &str, params: &Value) -> Option<&'static str> {
    if let Some(handled) = handled(id) {
        return Some(handled);
    }
    if id != BATCH {
        return None;
    }
    let steps = params.get("commands")?.as_array()?;
    steps.iter().find_map(|step| {
        let id = step.get("command")?.as_str()?;
        refused(id, step.get("params").unwrap_or(&Value::Null))
    })
}

/// The documents a session holds, as a command op found them: their uids in order, and the uid of
/// the one on show.
///
/// A uid names a document for good, because the engine gives every document it makes a new one. So
/// closing the canvas's document and opening another in its place, which leaves the count and the
/// index on show as they were, still reads as the change it is.
#[derive(Debug, PartialEq)]
struct Documents {
    uids: Vec<u64>,
    shown: Option<u64>,
}

impl Documents {
    fn of(session: &Session) -> Self {
        Self {
            uids: session.docs.iter().map(|doc| doc.uid).collect(),
            shown: session.ds().ok().map(|doc| doc.uid),
        }
    }

    /// Closes the documents opened since, and shows the one that was on show again.
    fn restore(&self, session: &mut Session) {
        session.docs.retain(|doc| self.uids.contains(&doc.uid));
        session.active = self
            .shown
            .and_then(|uid| session.docs.iter().position(|doc| doc.uid == uid))
            .unwrap_or(0);
    }
}

/// Runs command `id`, other than a batch. If it left other documents open, or another on show,
/// those are closed again and the canvas's document is shown: nothing but `file.close` removes the
/// canvas's own document outside a batch, and Sawit refuses that one.
///
/// Not run in a group of its own: `edit.undo` and `edit.redo` aren't undo steps, and a group would
/// record them as new ones and drop what can be redone.
fn single(board: &mut BoardDoc, id: &str, params: &Value) -> Result<Value, Failed> {
    let before = Documents::of(&board.session);
    let result = board.execute(id, params);
    if Documents::of(&board.session) != before {
        before.restore(&mut board.session);
        return Err(Failed::Switched(id.into()));
    }
    Ok(result?)
}

/// Runs a `command.batch` in a group that fails when its steps left other documents open, or
/// another on show: the engine then puts back the documents, the one on show and the canvas's own
/// document, so nothing of the batch stays.
///
/// The group takes the label the batch's own group does (`label`, else "Batch"), so a batch that
/// stays is the one undo step it always was.
fn batch(board: &mut BoardDoc, params: &Value) -> Result<Value, Failed> {
    let before = Documents::of(&board.session);
    let label = figcraft_engine::params::string(params, "label").unwrap_or_else(|| "Batch".into());
    let mut switched = false;
    let result = board.session.grouped(&label, |session| {
        let result = session.execute(BATCH, params)?;
        if Documents::of(session) != before {
            switched = true;
            return Err(EngineError::Disabled(
                BATCH.into(),
                "it would open or switch documents".into(),
            ));
        }
        Ok(result)
    });
    if switched {
        return Err(Failed::Switched(BATCH.into()));
    }
    Ok(result.map_err(BoardError::from)?)
}

/// The kinds of the `pointer` op, as an error lists them.
const POINTER_KINDS: &str = "down, move, up, double or hover";

fn pointer(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(name) = op.get("kind").and_then(Value::as_str) else {
        return failure(format!("a pointer op needs a kind: {POINTER_KINDS}"));
    };
    let kind = match name {
        "down" => PointerKind::Down,
        "move" => PointerKind::Move,
        "up" => PointerKind::Up,
        "double" => PointerKind::DoubleClick,
        "hover" => PointerKind::Hover,
        other => return failure(format!("{other:?} is no pointer kind: use {POINTER_KINDS}")),
    };
    let number = |key: &str| op.get(key).and_then(Value::as_f64);
    let (Some(x), Some(y)) = (number("x"), number("y")) else {
        return failure("a pointer op needs x and y, a point on the page");
    };
    let held = |key: &str| op.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mods = Mods {
        shift: held("shift"),
        alt: held("alt"),
        cmd: held("cmd"),
        ctrl: false,
    };
    let event = match board.session.ds() {
        Ok(ds) => input::page_pointer_event(kind, (x, y), &ds.view, mods),
        Err(error) => return failure(error),
    };
    let requests = match input::pointer(board, &event) {
        Ok(requests) => requests,
        Err(error) => return failure(error),
    };
    leave_for_the_view(board, &requests);
    let selection: Vec<String> = (board.session.selection().iter())
        .map(ToString::to_string)
        .collect();
    json!({"ok": true, "selection": selection, "tool": board.session.tool.tool.id()})
}

fn key(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(key) = op
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
    else {
        return failure(r#"a key op needs a key, like {"op": "key", "key": "Escape"}"#);
    };
    let held = |modifier: &str| op.get(modifier).and_then(Value::as_bool).unwrap_or(false);
    let mods = Mods {
        shift: held("shift"),
        alt: held("alt"),
        cmd: held("cmd"),
        ctrl: held("ctrl"),
    };
    let outcome = match input::route_key(board, key, mods) {
        Ok(outcome) => outcome,
        Err(error) => return failure(error),
    };
    let requests = std::mem::take(&mut board.session.ui_requests);
    leave_for_the_view(board, &requests);
    json!({"ok": true, "handled": outcome != KeyOutcome::Unhandled})
}

/// Carries out what an op's input asked of the view that needs none
/// ([`input::handle_requests`]), and leaves the rest in `session.ui_requests`, ahead of any made
/// since, as the same input from the window leaves them for the view.
fn leave_for_the_view(board: &mut BoardDoc, requests: &[UiRequest]) {
    let mut rest = input::handle_requests(board, requests);
    rest.append(&mut board.session.ui_requests);
    board.session.ui_requests = rest;
}

fn mermaid(board: &mut BoardDoc, op: &Value) -> Value {
    let source = op.get("source").and_then(Value::as_str);
    let Some(source) = source.filter(|source| !source.trim().is_empty()) else {
        return failure("mermaid: `source` must be a non-empty string");
    };
    match draw_mermaid(board, source, op) {
        Ok(applied) => {
            let mut reply = json!({"ok": true, "added": applied.ids.len(), "ids": applied.ids});
            if !applied.notes.is_empty() {
                reply["notes"] = json!(applied.notes);
            }
            reply
        }
        Err(error) => failure(format!("mermaid: {error}")),
    }
}

/// Renders `source`, makes board shapes of it and draws them where `op` says.
///
/// Rendering blocks, and the app answers ops on its UI thread, because the canvas answers
/// synchronously: well under a millisecond for a typical diagram, about 60 ms the first time,
/// while Pele loads, and at most 5 s a try, when `sawit_core::mermaid` stops a runaway diagram.
fn draw_mermaid(board: &mut BoardDoc, source: &str, op: &Value) -> Result<Applied, String> {
    let palette = Palette::default();
    let diagram = match render(source, &palette) {
        Ok(diagram) => diagram,
        // Double quotes trip up some diagram types, so a source that has them is tried once more
        // with single ones, as Excalidraw's own Mermaid import does. A diagram that fails both is
        // reported in the words of the first try.
        Err(first) if source.contains('"') => {
            render(&source.replace('"', "'"), &palette).map_err(|_| first)?
        }
        Err(first) => return Err(first),
    };
    let items = crate::mermaid::to_skeletons(&diagram)?;
    let (left, top, _, _) = skeleton::bounds(&items).ok_or("the diagram is empty")?;
    let (x, y) = diagram_corner(board, op).map_err(|error| error.to_string())?;
    skeleton::apply(board, &items, (x - left, y - top)).map_err(|error| error.to_string())
}

/// Where the `mermaid` op puts a diagram's top left corner: at the op's `x` and `y`. What it leaves
/// out is [`MERMAID_GAP`] right of what the page on show holds and level with its top, or 0 on an
/// empty page.
fn diagram_corner(board: &BoardDoc, op: &Value) -> Result<(f64, f64), BoardError> {
    let page = board.session.page()?;
    let held = board.doc()?.page_content_bounds(page);
    let (x, y) = held.map_or((0.0, 0.0), |held| (held.x1 + MERMAID_GAP, held.y0));
    let given = |key| op.get(key).and_then(Value::as_f64);
    Ok((given("x").unwrap_or(x), given("y").unwrap_or(y)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use figcraft_color::{Color, Paint};
    use figcraft_doc::{BoardItem, Document, NodeId};
    use serde_json::json;

    use super::*;
    use crate::doc::DocKind;
    use crate::policy;

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// The elements `get` lists, after checking that it answered.
    fn elements(board: &mut BoardDoc) -> Vec<Value> {
        let reply = run(board, &json!({"op": "get"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        reply["elements"].as_array().unwrap().clone()
    }

    /// Sends command op `id` with `params`.
    fn send(board: &mut BoardDoc, id: &str, params: Value) -> Value {
        run(
            board,
            &json!({"op": "command", "command": id, "params": params}),
        )
    }

    /// The uids of the session's documents, in order, and the uid of the one on show.
    fn documents(board: &BoardDoc) -> (Vec<u64>, u64) {
        let session = &board.session;
        (
            session.docs.iter().map(|doc| doc.uid).collect(),
            session.ds().unwrap().uid,
        )
    }

    /// A second board in `dir`, the kind of file `file.recover` opens. It has two stickies, so
    /// it can't be taken for a canvas that has one.
    fn other_board_file(dir: &std::path::Path) -> String {
        let mut other = BoardDoc::new_board("Other").unwrap();
        for x in [40, 400] {
            other
                .execute("board.sticky", &json!({"x": x, "y": 40, "text": "other"}))
                .unwrap();
        }
        let path = dir.join("other.figcraft");
        std::fs::write(&path, other.to_bytes().unwrap()).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// The answer to a command that would have left the canvas with another document.
    fn would_switch(id: &str) -> Value {
        json!({
            "ok": false,
            "error": format!(
                "{id} would open or switch documents; the canvas keeps its one document"
            ),
        })
    }

    /// The answer to a command that Sawit handles itself.
    fn handled_by_sawit(id: &str) -> Value {
        json!({"ok": false, "error": format!("{id} is handled by Sawit")})
    }

    #[test]
    fn get_lists_top_level_layers() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "one"}))
            .unwrap();
        board
            .execute("board.sticky", &json!({"x": 300, "y": 200, "text": "two"}))
            .unwrap();
        // Each sticky has a text layer of its own inside it, which `get` doesn't list.
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let top_level = doc.node(page).unwrap().children.clone();
        assert_eq!(top_level.len(), 2);
        assert!(
            top_level
                .iter()
                .all(|id| !doc.node(*id).unwrap().children.is_empty())
        );

        let elements = elements(&mut board);
        assert_eq!(elements.len(), 2, "{elements:?}");
        for element in &elements {
            let id = element["id"].as_str().expect("an id string");
            assert!(id.parse::<u64>().is_ok(), "{id} is not a number");
            assert_eq!(element["type"], json!("sticky"));
        }
        let ids: Vec<String> = top_level.iter().map(ToString::to_string).collect();
        assert_eq!(elements[0]["id"], json!(ids[0]));
        assert_eq!(elements[1]["id"], json!(ids[1]));
    }

    #[test]
    fn get_gives_each_layer_its_name_and_page_bounds() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 300, "y": 200, "text": "two"}))
            .unwrap();
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let id = doc.node(page).unwrap().children[0];
        let bounds = doc.world_bounds(id).unwrap();

        let elements = elements(&mut board);
        let element = &elements[0];
        assert_eq!(element["id"], json!(id.to_string()));
        assert_eq!(element["name"], json!(doc.node(id).unwrap().name));
        assert_eq!(element["x"], json!(bounds.x0));
        assert_eq!(element["y"], json!(bounds.y0));
        assert_eq!(element["width"], json!(bounds.width()));
        assert_eq!(element["height"], json!(bounds.height()));
        // Where the sticky was put, and a size it has.
        assert!((bounds.x0 - 300.0).abs() < 1e-6 && (bounds.y0 - 200.0).abs() < 1e-6);
        assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
    }

    #[test]
    fn get_lists_only_the_page_on_show() {
        let mut board = board();
        assert!(elements(&mut board).is_empty());
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0}))
            .unwrap();
        assert_eq!(elements(&mut board).len(), 1);
        // A new page is empty and on show; the sticky is on the first page.
        board.execute("page.add", &json!({})).unwrap();
        assert!(elements(&mut board).is_empty());
        board.execute("page.select", &json!({"index": 0})).unwrap();
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn command_runs_engine_commands() {
        let mut board = board();
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(!reply["result"]["id"].is_null(), "{reply}");
        // It ran on the board.
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn command_params_are_optional() {
        let mut board = board();
        let zoom = |board: &BoardDoc| board.session.ds().unwrap().view.zoom;
        let before = zoom(&board);
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "view.zoomIn"}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let once = zoom(&board);
        assert!(once > before);
        // `null` is no params either.
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "view.zoomIn", "params": null}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(zoom(&board) > once);
    }

    #[test]
    fn command_errors_are_reported() {
        let mut board = board();
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "no.such.command"}),
        );
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("no.such.command"), "{error}");
        assert!(reply.get("result").is_none());

        // The engine refusing a command's params is reported too, with its own words, and leaves
        // the board alone.
        let before = board.doc().unwrap();
        for params in [
            json!({"x": 0, "y": 0, "color": "not a colour"}),
            json!({"x": 0, "y": 0, "no_such_param": 1}),
        ] {
            let reply = run(
                &mut board,
                &json!({"op": "command", "command": "board.sticky", "params": params}),
            );
            assert_eq!(reply["ok"], json!(false), "{params}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains("board.sticky"), "{error}");
        }
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert!(elements(&mut board).is_empty());
    }

    #[test]
    fn a_command_op_without_a_command_id_is_reported() {
        let mut board = board();
        for op in [
            json!({"op": "command"}),
            json!({"op": "command", "command": 5}),
            json!({"op": "command", "params": {"x": 0, "y": 0}}),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains("command"), "{error}");
        }
        assert!(elements(&mut board).is_empty());
    }

    #[test]
    fn commands_that_switch_or_write_documents_are_refused() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0}))
            .unwrap();
        let before = board.doc().unwrap();
        let revision = board.revision();
        for id in [
            "file.new",
            "file.newBoard",
            "file.newFromTemplate",
            "file.open",
            "file.openRecent",
            "file.close",
            "file.duplicate",
            "file.save",
            "file.saveAs",
            "window.new",
            "doc.nextTab",
            "doc.previousTab",
        ] {
            let reply = run(&mut board, &json!({"op": "command", "command": id}));
            assert_eq!(
                reply,
                json!({"ok": false, "error": format!("{id} is handled by Sawit")})
            );
        }
        // None of them reached the engine: the board is the one it was.
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.revision(), revision);
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn refused_commands_are_ones_the_policy_hides_or_hands_to_sawit() {
        for id in HANDLED_BY_SAWIT {
            assert!(
                policy::is_hidden(id) || policy::is_ui(id),
                "{id} is neither hidden nor Sawit's own in the command policy"
            );
        }
    }

    #[test]
    fn commands_that_sawit_handles_are_refused_as_batch_steps_too() {
        let mut board = board();
        let before = board.doc().unwrap();
        for id in HANDLED_BY_SAWIT {
            let reply = send(
                &mut board,
                "command.batch",
                json!({"commands": [
                    {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                    {"command": id},
                ]}),
            );
            assert_eq!(reply, handled_by_sawit(id));
        }
        // Nothing ran, not even the sticky that came before the refused step.
        assert_eq!(board.session.docs.len(), 1);
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));
        assert!(elements(&mut board).is_empty());
    }

    #[test]
    fn a_save_inside_nested_batches_is_refused_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("copy.figcraft");
        let mut board = board();
        let before = board.doc().unwrap();

        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": "command.batch", "params": {"commands": [
                    {"command": "file.saveAs", "params": {"path": target}},
                ]}},
            ]}),
        );

        assert_eq!(reply, handled_by_sawit("file.saveAs"));
        assert!(!target.exists(), "the save wrote {}", target.display());
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        // The canvas's document was never given a path, so file.revert has nothing to read.
        assert_eq!(board.session.ds().unwrap().path, None);
    }

    #[test]
    fn a_recovered_file_would_replace_the_canvas_and_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_board_file(dir.path());
        let mut board = board();
        let reply = send(
            &mut board,
            "board.sticky",
            json!({"x": 0, "y": 0, "text": "mine"}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        let reply = send(&mut board, "file.recover", json!({"path": other}));

        assert_eq!(reply, would_switch("file.recover"));
        // The canvas holds its one document, on show and as it was: `get` lists its one sticky,
        // not the two of the recovered file.
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(documents(&board), documents_before);
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn a_batch_that_opens_a_document_is_undone_whole() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_board_file(dir.path());
        let mut board = board();
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": "file.recover", "params": {"path": other}},
            ]}),
        );

        assert_eq!(reply, would_switch("command.batch"));
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(documents(&board), documents_before);
        // The sticky the batch made before it opened the file went with it: the canvas is the
        // very document it was, and its history has no step for the batch.
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
        assert!(elements(&mut board).is_empty());
        assert_eq!(board.session.undo_depth(), (0, 0));
    }

    #[test]
    fn a_batch_that_opens_a_document_and_switches_back_is_refused_too() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_board_file(dir.path());
        let mut board = board();
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        // At its end the canvas's document is on show again, but a second one is open.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "file.recover", "params": {"path": other}},
                {"command": "doc.switch", "params": {"index": 0}},
            ]}),
        );

        assert_eq!(reply, would_switch("command.batch"));
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(documents(&board), documents_before);
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
    }

    #[test]
    fn other_commands_that_open_a_document_are_refused_too() {
        let mut board = board();
        let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        // A version of the canvas, for `version.duplicate` to open.
        let reply = send(&mut board, "file.saveVersion", json!({"name": "First"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        for (id, params) in [
            ("version.duplicate", json!({"index": 0})),
            ("branch.create", json!({"name": "Try it"})),
        ] {
            let reply = send(&mut board, id, params);
            assert_eq!(reply, would_switch(id));
            assert_eq!(documents(&board), documents_before, "{id}");
            assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()), "{id}");
        }
        assert_eq!(elements(&mut board).len(), 1);
    }

    /// How many versions and branches the canvas's file has: what its next save would write of
    /// them.
    fn history(board: &BoardDoc) -> (usize, usize) {
        let ds = board.session.ds().unwrap();
        (ds.versions.len(), ds.branches.len())
    }

    #[test]
    fn a_failed_command_op_leaves_no_version_or_branch_to_save() {
        let mut board = board();
        let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        assert_eq!(reply["ok"], json!(true), "{reply}");

        // `branch.create` adds its branch (two copies of the document) to the canvas's file
        // before it opens it in a tab. The guard closes the tab, and the branch goes with it.
        let reply = send(&mut board, "branch.create", json!({"name": "Try it"}));
        assert_eq!(reply, would_switch("branch.create"));
        assert_eq!(history(&board), (0, 0));

        // A batch the guard refuses: what its steps added to the file goes with it too.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "file.saveVersion", "params": {"name": "First"}},
                {"command": "branch.create", "params": {"name": "Try it"}},
            ]}),
        );
        assert_eq!(reply, would_switch("command.batch"));
        assert_eq!(history(&board), (0, 0));

        // So does a batch that fails on a step of its own: none of it is done.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "file.saveVersion", "params": {"name": "First"}},
                {"command": "no.such.command"},
            ]}),
        );
        assert_eq!(reply["ok"], json!(false), "{reply}");
        assert_eq!(history(&board), (0, 0));

        // A command that goes through keeps what it adds.
        let reply = send(&mut board, "file.saveVersion", json!({"name": "Kept"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(history(&board), (1, 0));
        // And a refusal after it leaves that version where it is.
        let reply = send(&mut board, "branch.create", json!({}));
        assert_eq!(reply, would_switch("branch.create"));
        assert_eq!(history(&board), (1, 0));
        assert_eq!(board.session.ds().unwrap().versions[0].name, "Kept");
    }

    /// A session whose one document is a board called "Canvas", made by the engine itself.
    fn session_with_a_canvas() -> Session {
        let mut session = Session::empty();
        session
            .execute("file.newBoard", &json!({"name": "Canvas"}))
            .unwrap();
        session
    }

    #[test]
    fn documents_are_told_apart_by_uid_not_by_count_and_position() {
        let mut session = session_with_a_canvas();
        let before = Documents::of(&session);
        assert_eq!(Documents::of(&session), before);

        // Close the canvas's document and open another in its place: still one document, and
        // still the first on show.
        session
            .execute("file.newBoard", &json!({"name": "Other"}))
            .unwrap();
        session
            .execute("file.close", &json!({"index": 0, "force": true}))
            .unwrap();
        assert_eq!((session.docs.len(), session.active), (1, 0));
        assert_ne!(Documents::of(&session), before);
    }

    #[test]
    fn restoring_shows_the_canvas_wherever_it_now_is() {
        let mut session = session_with_a_canvas();
        let before = Documents::of(&session);

        // Two more documents, the canvas's moved behind them, the first of them on show.
        for name in ["One", "Two"] {
            session
                .execute("file.newBoard", &json!({"name": name}))
                .unwrap();
        }
        session
            .execute("file.moveTab", &json!({"from": 0, "to": 2}))
            .unwrap();
        session.execute("doc.switch", &json!({"index": 0})).unwrap();
        assert_eq!((session.docs.len(), session.active), (3, 0));
        assert_ne!(Documents::of(&session), before);

        before.restore(&mut session);
        assert_eq!(Documents::of(&session), before);
        assert_eq!((session.docs.len(), session.active), (1, 0));
        assert_eq!(session.doc().unwrap().name, "Canvas");
    }

    #[test]
    fn a_failure_that_opens_nothing_keeps_the_engines_own_words() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.figcraft");
        let mut board = board();
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        let reply = send(&mut board, "file.recover", json!({"path": missing}));
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("missing.figcraft"), "{error}");
        assert!(!error.contains("would open or switch"), "{error}");

        // A step that fails rolls the batch back, and the step's error is what is answered.
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": "no.such.command"},
            ]}),
        );
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("no.such.command"), "{error}");
        assert!(!error.contains("would open or switch"), "{error}");

        assert_eq!(documents(&board), documents_before);
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));
    }

    #[test]
    fn command_ops_undo_and_redo_what_an_op_did() {
        let mut board = board();
        let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(elements(&mut board).len(), 1);

        let reply = send(&mut board, "edit.undo", json!({}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(elements(&mut board).is_empty());

        // Undoing isn't an edit of its own: the step can be redone.
        let reply = send(&mut board, "edit.redo", json!({}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn a_batch_is_one_undo_step_with_its_own_label() {
        let mut board = board();
        let stickies = |label: Option<&str>| {
            let steps = json!([
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": "board.sticky", "params": {"x": 300, "y": 0}},
            ]);
            match label {
                Some(label) => json!({"label": label, "commands": steps}),
                None => json!({"commands": steps}),
            }
        };

        let reply = send(&mut board, "command.batch", stickies(None));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(reply["result"].as_array().map(Vec::len), Some(2));
        assert_eq!(board.session.undo_depth(), (1, 0));
        assert_eq!(board.session.undo_labels(5), ["Batch"]);

        let reply = send(&mut board, "command.batch", stickies(Some("Two more")));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(board.session.undo_depth(), (2, 0));
        assert_eq!(board.session.undo_labels(5), ["Two more", "Batch"]);
        assert_eq!(elements(&mut board).len(), 4);

        // One undo takes back one batch.
        let reply = send(&mut board, "edit.undo", json!({}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(elements(&mut board).len(), 2);
    }

    #[test]
    fn a_batch_that_opens_nothing_runs_as_the_engine_alone_would_run_it() {
        let sticky = json!({"command": "board.sticky", "params": {"x": 0, "y": 0}});
        let (undo, redo) = (
            json!({"command": "edit.undo"}),
            json!({"command": "edit.redo"}),
        );
        let nested = json!({"command": "command.batch", "params": {
            "label": "Inner",
            "commands": [sticky, sticky],
        }});
        let broken = json!({"command": "no.such.command"});
        let batches = [
            json!({"commands": []}),
            json!({"commands": [sticky, sticky]}),
            json!({"label": "Named", "commands": [sticky]}),
            json!({"commands": [sticky, undo]}),
            json!({"commands": [sticky, undo, redo]}),
            json!({"commands": [nested, sticky]}),
            json!({"commands": [sticky, broken]}),
            json!({"onError": "continue", "commands": [sticky, broken, sticky]}),
        ];

        for batch in batches {
            let (mut through_the_op, mut engine_alone) = (board(), board());
            let reply = send(&mut through_the_op, "command.batch", batch.clone());
            let expected = match engine_alone.execute("command.batch", &batch) {
                Ok(result) => json!({"ok": true, "result": result}),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            };
            assert_eq!(reply, expected, "{batch}");

            // The same undo steps (and what can be redone), history of commands and layers.
            let state = |board: &mut BoardDoc| {
                (
                    board.session.undo_depth(),
                    board.session.undo_labels(10),
                    board.session.journal.clone(),
                    elements(board).len(),
                )
            };
            assert_eq!(
                state(&mut through_the_op),
                state(&mut engine_alone),
                "{batch}"
            );
        }
    }

    /// A design file, which takes no board objects.
    fn design() -> BoardDoc {
        let bytes = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
        BoardDoc::from_bytes(&bytes).unwrap()
    }

    /// Sends `op`, which has to answer `{ok: true, ...}`.
    fn ask(board: &mut BoardDoc, op: Value) -> Value {
        let reply = run(board, &op);
        assert_eq!(reply["ok"], json!(true), "{op}: {reply}");
        reply
    }

    /// Sends `op`, which has to answer `{ok: false, error}`, and gives the error.
    fn refusal(board: &mut BoardDoc, op: Value) -> String {
        let reply = run(board, &op);
        assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
        let error = reply["error"].as_str().expect("an error string");
        error.to_string()
    }

    /// Adds `elements` with the `add` op and gives the ids it answered.
    fn add(board: &mut BoardDoc, elements: Value) -> Vec<String> {
        let reply = ask(board, json!({"op": "add", "elements": elements}));
        let ids = reply["ids"].as_array().expect("ids");
        ids.iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect()
    }

    /// The element `get` lists under `id`.
    fn listed(board: &mut BoardDoc, id: &str) -> Value {
        let found = elements(board).into_iter().find(|e| e["id"] == json!(id));
        found.unwrap_or_else(|| panic!("{id} is not listed"))
    }

    /// The layers of the page on show, labels among them.
    fn layers_on_page(board: &BoardDoc) -> usize {
        let ds = board.session.ds().unwrap();
        ds.doc.node(ds.page).unwrap().children.len()
    }

    /// How many connector labels the document holds.
    fn labels(board: &BoardDoc) -> usize {
        let doc = board.doc().unwrap();
        doc.nodes()
            .filter(|n| matches!(n.board, Some(BoardItem::Label { .. })))
            .count()
    }

    /// The first solid fill and stroke of layer `id`, as `#RRGGBB`.
    fn paints(board: &BoardDoc, id: &str) -> (Option<String>, Option<String>) {
        let doc = board.doc().unwrap();
        let node = doc.node(id.parse().unwrap()).unwrap();
        let first = |paints: &[Paint]| paints.iter().find_map(Paint::solid_color);
        let (fill, stroke) = (first(&node.fills), first(&node.strokes));
        (fill.map(Color::to_css_hex), stroke.map(Color::to_css_hex))
    }

    #[test]
    fn get_summarises_types_and_text() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "sticky", "id": "s", "x": 0, "y": 0, "text": "hi"},
                {"type": "diamond", "id": "d", "x": 400, "y": 0},
                {"type": "arrow", "start": {"id": "s"}, "end": {"id": "d"}, "label": {"text": "yes"}},
            ]),
        );

        // Three elements: the connector's label is a layer of its own, but not an element (P7-C4).
        assert_eq!(layers_on_page(&board), 4);
        let elements = elements(&mut board);
        assert_eq!(elements.len(), 3, "{elements:?}");
        let types: Vec<&str> = elements
            .iter()
            .map(|e| e["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["sticky", "diamond", "connector"]);
        let listed_ids: Vec<&str> = elements.iter().map(|e| e["id"].as_str().unwrap()).collect();
        assert_eq!(listed_ids, ids);

        assert_eq!(elements[0]["text"], json!("hi"));
        assert!(elements[1].get("text").is_none(), "{}", elements[1]);
        // The connector says what it joins, and its text is its label's.
        assert_eq!(elements[2]["from"], json!(ids[0]));
        assert_eq!(elements[2]["to"], json!(ids[1]));
        assert_eq!(elements[2]["text"], json!("yes"));
        for element in &elements {
            for key in ["x", "y", "width", "height"] {
                assert!(element[key].is_number(), "{key} of {element}");
            }
        }
    }

    #[test]
    fn add_returns_ids_in_order() {
        let mut board = board();
        // Made in another order (shapes before text, text before what joins them), answered in
        // this one.
        let reply = ask(
            &mut board,
            json!({"op": "add", "elements": [
                {"type": "arrow", "start": {"id": "a"}, "end": {"id": "b"}},
                {"type": "text", "x": 0, "y": -50, "text": "Title"},
                {"type": "rectangle", "id": "b", "x": 300, "y": 0, "width": 100, "height": 60},
                {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 100, "height": 60},
            ]}),
        );
        assert_eq!(reply["added"], json!(4));
        assert!(reply.get("notes").is_none(), "{reply}");
        let ids: Vec<String> = reply["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids.len(), 4);
        let at = |board: &mut BoardDoc, id: &String| {
            let element = listed(board, id);
            (element["type"].clone(), element["x"].clone())
        };
        assert_eq!(at(&mut board, &ids[0]).0, json!("connector"));
        assert_eq!(at(&mut board, &ids[1]), (json!("text"), json!(0.0)));
        assert_eq!(at(&mut board, &ids[2]), (json!("rectangle"), json!(300.0)));
        assert_eq!(at(&mut board, &ids[3]), (json!("rectangle"), json!(0.0)));
        // The arrow joins the two rectangles by the ids the agent gave them.
        let connector = listed(&mut board, &ids[0]);
        assert_eq!(connector["from"], json!(ids[3]));
        assert_eq!(connector["to"], json!(ids[2]));
    }

    #[test]
    fn add_notes_what_it_skips() {
        let mut board = board();
        let reply = ask(
            &mut board,
            json!({"op": "add", "elements": [
                {"type": "hologram", "x": 0, "y": 0},
                {"type": "rectangle", "x": 0, "y": 0},
            ]}),
        );
        // Skipped with a note, not an error, and the rest is drawn.
        assert_eq!(reply["added"], json!(1));
        assert_eq!(reply["ids"].as_array().map(Vec::len), Some(1));
        let notes = reply["notes"].as_array().expect("notes");
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].as_str().unwrap().contains("hologram"), "{notes:?}");
        assert_eq!(elements(&mut board).len(), 1);

        // Nothing to add adds nothing.
        let reply = ask(&mut board, json!({"op": "add", "elements": []}));
        assert_eq!(reply, json!({"ok": true, "added": 0, "ids": []}));
    }

    #[test]
    fn add_reports_malformed_elements() {
        let mut board = board();
        let before = board.doc().unwrap();
        for op in [
            json!({"op": "add"}),
            json!({"op": "add", "elements": null}),
            json!({"op": "add", "elements": "rectangle"}),
            json!({"op": "add", "elements": {"type": "rectangle"}}),
            json!({"op": "add", "elements": ["rectangle"]}),
            json!({"op": "add", "elements": [{"type": "rectangle", "x": "far away"}]}),
            json!({"op": "add", "elements": [{"type": "rectangle", "x": 0, "y": 0}, 5]}),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.contains("elements"), "{op}: {error}");
        }
        // The engine refusing an element fails the whole call, and says which one.
        let error = refusal(
            &mut board,
            json!({"op": "add", "elements": [
                {"type": "rectangle", "x": 0, "y": 0},
                {"type": "rectangle", "x": 0, "y": 0, "width": 0},
            ]}),
        );
        assert!(error.contains("elements[1]"), "{error}");
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));
    }

    #[test]
    fn add_to_a_design_file_is_refused() {
        let mut design = design();
        let before = design.doc().unwrap();
        let error = refusal(
            &mut design,
            json!({"op": "add", "elements": [{"type": "rectangle", "x": 0, "y": 0}]}),
        );
        assert!(error.contains("board"), "{error}");
        assert!(Arc::ptr_eq(&before, &design.doc().unwrap()));
    }

    #[test]
    fn update_moves_and_relabels() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([{"type": "sticky", "id": "s", "x": 0, "y": 20, "text": "Old"}]),
        );

        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": ids[0], "x": 500, "text": "New"}]}),
        );

        assert_eq!(reply, json!({"ok": true, "updated": 1, "missing": []}));
        let element = listed(&mut board, &ids[0]);
        assert_eq!(element["x"], json!(500.0));
        // What the element didn't name stays.
        assert_eq!(element["y"], json!(20.0));
        assert_eq!(element["text"], json!("New"));
    }

    #[test]
    fn update_reports_missing() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([{"type": "rectangle", "id": "box", "x": 0, "y": 0}]),
        );
        let before = board.doc().unwrap();

        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "nope", "x": 5}]}),
        );
        assert_eq!(
            reply,
            json!({"ok": true, "updated": 0, "missing": ["nope"]})
        );
        // Nothing changed, so there is no step to undo.
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (1, 0));

        // What is there is updated, what isn't is named, in the order it was given. The page
        // is no element, though its node id is a number like the others'.
        let page = board.doc().unwrap().first_page().unwrap().to_string();
        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": "gone", "x": 1}, {"id": "box", "x": 5}, {"id": page, "x": 5}, {"id": ids[0], "y": 7},
            ]}),
        );
        assert_eq!(reply["updated"], json!(2));
        assert_eq!(reply["missing"], json!(["gone", page]));
        let element = listed(&mut board, &ids[0]);
        assert_eq!(
            (element["x"].clone(), element["y"].clone()),
            (json!(5.0), json!(7.0))
        );
    }

    #[test]
    fn update_changes_size_text_name_and_colours() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "box", "x": 0, "y": 0, "width": 100, "height": 60,
                 "label": {"text": "Old"}},
                {"type": "sticky", "id": "note", "x": 300, "y": 0, "text": "Old"},
                {"type": "text", "id": "title", "x": 0, "y": -100, "text": "Old"},
                {"type": "frame", "id": "area", "x": 0, "y": 300, "width": 200, "height": 100,
                 "name": "Old"},
                {"type": "arrow", "id": "link", "start": {"id": "box"}, "end": {"id": "note"}},
            ]),
        );

        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": "box", "x": 20, "y": 40, "width": 140, "height": 80, "text": "Box",
                 "backgroundColor": "#2f9e44", "strokeColor": "#e03131"},
                {"id": "note", "label": {"text": "Note"}, "backgroundColor": "pink"},
                {"id": "title", "text": "Title", "strokeColor": "#e03131"},
                {"id": "area", "name": "Area", "width": 250},
                {"id": "link", "text": "uses", "strokeColor": "#1971c2"},
            ]}),
        );
        assert_eq!(reply, json!({"ok": true, "updated": 5, "missing": []}));

        let shape = listed(&mut board, &ids[0]);
        assert_eq!(
            [&shape["x"], &shape["y"], &shape["width"], &shape["height"]],
            [&json!(20.0), &json!(40.0), &json!(140.0), &json!(80.0)]
        );
        assert_eq!(shape["text"], json!("Box"));
        assert_eq!(shape["color"], json!("#2F9E44"));
        // The stroke is the outline's, the layer inside the shape.
        let doc = board.doc().unwrap();
        let outline = doc.node(ids[0].parse().unwrap()).unwrap().children[0];
        assert_eq!(
            paints(&board, &outline.to_string()).1.as_deref(),
            Some("#E03131")
        );

        let sticky = listed(&mut board, &ids[1]);
        assert_eq!(sticky["text"], json!("Note"));
        assert_eq!(sticky["color"], json!("#F7B9DA"));

        // A text layer's colour is its `strokeColor` (P7-C24).
        let text = listed(&mut board, &ids[2]);
        assert_eq!(text["text"], json!("Title"));
        assert_eq!(text["color"], json!("#E03131"));
        assert_eq!(paints(&board, &ids[2]).1, None);

        let frame = listed(&mut board, &ids[3]);
        assert_eq!(frame["name"], json!("Area"));
        assert_eq!(frame["width"], json!(250.0));

        // A connector's text is a label, made on the way.
        let connector = listed(&mut board, &ids[4]);
        assert_eq!(connector["text"], json!("uses"));
        assert_eq!(connector["color"], json!("#1971C2"));
        assert_eq!(labels(&board), 1);
        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "link", "text": ""}]}),
        );
        assert_eq!(reply["updated"], json!(1));
        assert!(listed(&mut board, &ids[4]).get("text").is_none());
        assert_eq!(labels(&board), 0);
    }

    #[test]
    fn update_colours_by_the_rules_apply_uses() {
        // One helper colours what `add` draws and what `update` changes (P7-C12), so naming the
        // colours when drawing and naming them afterwards give the same document.
        let bare = json!([
            {"type": "rectangle", "x": 0, "y": 0},
            {"type": "rectangle", "x": 300, "y": 0},
            {"type": "sticky", "x": 600, "y": 0},
            {"type": "text", "x": 0, "y": 300, "text": "t"},
            {"type": "arrow", "x": 0, "y": 400, "width": 100, "height": 0},
            {"type": "freedraw", "x": 0, "y": 500, "points": [[0, 0], [30, 40], [60, 0]]},
            {"type": "frame", "x": 0, "y": 600, "width": 200, "height": 100},
        ]);
        let colours = [
            json!({"backgroundColor": "#74c0fc", "strokeColor": "#e03131"}),
            json!({"backgroundColor": "transparent", "strokeColor": "#2f9e44"}),
            json!({"backgroundColor": "violet"}),
            json!({"backgroundColor": "#ff0000", "strokeColor": "#1971c2"}),
            // A connector has no fill: the background would recolour its line, so it is ignored.
            json!({"backgroundColor": "#ffc9c9", "strokeColor": "#9c36b5"}),
            json!({"backgroundColor": "#ffc9c9", "strokeColor": "#e8590c"}),
            json!({"backgroundColor": "#d3f9d8", "strokeColor": "#2f9e44"}),
        ];

        let mut drawn = board();
        let mut elements = bare.clone();
        for (element, colours) in elements.as_array_mut().unwrap().iter_mut().zip(&colours) {
            element
                .as_object_mut()
                .unwrap()
                .extend(colours.as_object().unwrap().clone());
        }
        add(&mut drawn, elements);

        let mut changed = board();
        let ids = add(&mut changed, bare);
        let patches: Vec<Value> = ids
            .iter()
            .zip(&colours)
            .map(|(id, colours)| {
                let mut patch = colours.clone();
                patch["id"] = json!(id);
                patch
            })
            .collect();
        ask(&mut changed, json!({"op": "update", "elements": patches}));

        let (left, right) = (drawn.doc().unwrap(), changed.doc().unwrap());
        for (a, b) in left.nodes().zip(right.nodes()) {
            assert_eq!(a, b, "layer {} ({}) differs", a.id, a.name);
        }
        assert!(
            *left == *right,
            "the layers are alike but the documents differ"
        );
        // And the colours are not nothing: the connector's line is its stroke, not its background.
        assert_eq!(paints(&changed, &ids[4]).1.as_deref(), Some("#9C36B5"));
        assert_eq!(paints(&changed, &ids[1]).0, None);
    }

    #[test]
    fn update_places_layers_in_page_coordinates() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "frame", "id": "area", "x": 100, "y": 100, "width": 400, "height": 300},
                {"type": "rectangle", "id": "inner", "x": 150, "y": 160, "width": 50, "height": 50,
                 "frameId": "area"},
            ]),
        );
        let inner: NodeId = ids[1].parse().unwrap();
        let doc = board.doc().unwrap();
        assert_eq!(
            doc.node(inner).unwrap().parent,
            Some(ids[0].parse().unwrap())
        );
        // It is inside the frame, so `get` lists the frame alone.
        assert_eq!(elements(&mut board).len(), 1);

        ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "inner", "x": 300, "width": 80}]}),
        );

        // Where `add` put it is where `update` puts it: the page's coordinates, not the frame's.
        let bounds = board.doc().unwrap().world_bounds(inner).unwrap();
        assert_eq!(
            [bounds.x0, bounds.y0, bounds.width(), bounds.height()],
            [300.0, 160.0, 80.0, 50.0]
        );
    }

    #[test]
    fn update_finds_layers_by_agent_id_and_by_node_id() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 0, "y": 200},
                {"type": "rectangle", "id": "c", "x": 0, "y": 400},
            ]),
        );
        let number: u64 = ids[2].parse().unwrap();

        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": "a", "x": 11}, {"id": ids[1], "x": 22}, {"id": number, "x": 33},
            ]}),
        );
        assert_eq!(reply, json!({"ok": true, "updated": 3, "missing": []}));
        let xs: Vec<Value> = ids
            .iter()
            .map(|id| listed(&mut board, id)["x"].clone())
            .collect();
        assert_eq!(xs, [json!(11.0), json!(22.0), json!(33.0)]);

        // An agent's id wins over a node id that looks the same (a node is called "1" too).
        add(
            &mut board,
            json!([{"type": "rectangle", "id": ids[0], "x": 0, "y": 600}]),
        );
        ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": ids[0], "x": 99}]}),
        );
        assert_eq!(listed(&mut board, &ids[0])["x"], json!(11.0));
        let theirs = elements(&mut board)
            .into_iter()
            .find(|e| e["y"] == json!(600.0))
            .unwrap();
        assert_eq!(theirs["x"], json!(99.0));
    }

    #[test]
    fn update_is_one_undo_step_and_fails_whole() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 0, "y": 200},
            ]),
        );
        ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "a", "x": 50, "text": "A"}, {"id": "b", "x": 60}]}),
        );
        // Two elements, three changes, one step.
        assert_eq!(board.session.undo_depth(), (2, 0));
        assert_eq!(board.session.undo_labels(1), ["Update elements"]);
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(listed(&mut board, &ids[0])["x"], json!(0.0));
        assert_eq!(listed(&mut board, &ids[1])["x"], json!(0.0));
        assert!(listed(&mut board, &ids[0]).get("text").is_none());

        // What the engine refuses fails the call whole, naming the element: the first element's
        // change is not kept.
        let before = board.doc().unwrap();
        let steps = board.session.undo_depth();
        let error = refusal(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": "a", "x": 50}, {"id": "b", "backgroundColor": "not a colour"},
            ]}),
        );
        assert!(
            error.contains("elements[1]") && error.contains("b"),
            "{error}"
        );
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), steps);
    }

    #[test]
    fn update_reports_malformed_input() {
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "rectangle", "id": "a", "x": 0, "y": 0}]),
        );
        let before = board.doc().unwrap();
        for op in [
            json!({"op": "update"}),
            json!({"op": "update", "elements": "a"}),
            json!({"op": "update", "elements": [{"x": 5}]}),
            json!({"op": "update", "elements": [{"id": ""}]}),
            json!({"op": "update", "elements": [{"id": null, "x": 5}]}),
            json!({"op": "update", "elements": ["a"]}),
            json!({"op": "update", "elements": [{"id": "a", "x": "far"}]}),
            json!({"op": "update", "elements": [{"id": "a", "text": 5}]}),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.contains("elements"), "{op}: {error}");
        }
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    }

    #[test]
    fn update_notes_text_that_a_layer_cannot_take() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "freedraw", "x": 0, "y": 0, "points": [[0, 0], [30, 40], [60, 0]]},
                {"type": "rectangle", "x": 200, "y": 0},
            ]),
        );
        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [
                {"id": ids[0], "text": "ink can't say it", "x": 5}, {"id": ids[1], "text": "a shape can"},
            ]}),
        );
        // It is not an error: the rest of the element, and the others, are updated.
        assert_eq!(reply["updated"], json!(2));
        let notes = reply["notes"].as_array().expect("notes");
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].as_str().unwrap().contains("elements[0]"),
            "{notes:?}"
        );
        assert_eq!(listed(&mut board, &ids[0])["x"], json!(5.0));
        assert_eq!(listed(&mut board, &ids[1])["text"], json!("a shape can"));
    }

    #[test]
    fn the_users_selection_survives_update_and_delete() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 300, "y": 0},
                {"type": "rectangle", "id": "c", "x": 600, "y": 0},
            ]),
        );
        let nodes: Vec<NodeId> = ids.iter().map(|id| id.parse().unwrap()).collect();
        board.session.set_selection(vec![nodes[0], nodes[2]]);

        ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "b", "x": 5, "text": "B", "backgroundColor": "red"}]}),
        );
        assert_eq!(board.session.selection(), [nodes[0], nodes[2]]);

        // What is deleted leaves the selection, and what isn't deleted stays in it.
        ask(&mut board, json!({"op": "delete", "ids": ["b", "c"]}));
        assert_eq!(board.session.selection(), [nodes[0]]);
    }

    #[test]
    fn delete_removes_connector_label() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 400, "y": 0},
                {"type": "arrow", "id": "ab", "start": {"id": "a"}, "end": {"id": "b"},
                 "label": {"text": "yes"}},
            ]),
        );
        assert_eq!(labels(&board), 1);
        assert_eq!(layers_on_page(&board), 4);

        let reply = ask(&mut board, json!({"op": "delete", "ids": [ids[2]]}));

        assert_eq!(reply, json!({"ok": true, "deleted": 1, "missing": []}));
        // The label went with its connector, and the shapes it joined stay.
        assert_eq!(labels(&board), 0);
        assert_eq!(layers_on_page(&board), 2);
        let types: Vec<String> = elements(&mut board)
            .iter()
            .map(|e| e["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(types, ["rectangle", "rectangle"]);
        // One step takes it back, label and all.
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!((labels(&board), layers_on_page(&board)), (1, 4));
    }

    #[test]
    fn delete_reports_missing_and_counts_what_it_found() {
        let mut board = board();
        let ids = add(
            &mut board,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 300, "y": 0},
                {"type": "rectangle", "id": "c", "x": 600, "y": 0},
            ]),
        );
        let page = board.doc().unwrap().first_page().unwrap().to_string();
        let number: u64 = ids[2].parse().unwrap();

        // By an agent's id, by a node id, twice the same layer, a layer that isn't there, and the
        // page, which is no element.
        let reply = ask(
            &mut board,
            json!({"op": "delete", "ids": ["a", ids[0], "nope", page, number, "b"]}),
        );

        assert_eq!(reply["deleted"], json!(3));
        assert_eq!(reply["missing"], json!(["nope", page]));
        assert!(elements(&mut board).is_empty());
        assert_eq!(board.doc().unwrap().pages.len(), 1);

        // Nothing found, nothing done.
        let before = board.doc().unwrap();
        let reply = ask(&mut board, json!({"op": "delete", "ids": ["x", "y"]}));
        assert_eq!(
            reply,
            json!({"ok": true, "deleted": 0, "missing": ["x", "y"]})
        );
        let reply = ask(&mut board, json!({"op": "delete", "ids": []}));
        assert_eq!(reply, json!({"ok": true, "deleted": 0, "missing": []}));
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    }

    #[test]
    fn delete_never_falls_back_to_the_selection() {
        // The engine's delete takes the selection when it is given no layers.
        let mut board = board();
        let ids = add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
        board.session.set_selection(vec![ids[0].parse().unwrap()]);
        for ids in [json!([]), json!(["nope"])] {
            ask(&mut board, json!({"op": "delete", "ids": ids}));
        }
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn delete_reports_malformed_input() {
        let mut board = board();
        add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
        for op in [
            json!({"op": "delete"}),
            json!({"op": "delete", "ids": "a"}),
            json!({"op": "delete", "ids": ["a", {"id": "b"}]}),
            json!({"op": "delete", "ids": [null]}),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.contains("ids"), "{op}: {error}");
        }
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn clear_is_one_undo_step() {
        let mut board = board();
        add(
            &mut board,
            json!([
                {"type": "sticky", "id": "s", "x": 0, "y": 0, "text": "hi"},
                {"type": "ellipse", "id": "e", "x": 400, "y": 0},
                {"type": "arrow", "start": {"id": "s"}, "end": {"id": "e"}, "label": {"text": "yes"}},
                {"type": "freedraw", "x": 0, "y": 500, "points": [[0, 0], [30, 40], [60, 0]]},
            ]),
        );
        let (layers, steps) = (layers_on_page(&board), board.session.undo_depth().0);
        assert_eq!((elements(&mut board).len(), layers), (4, 5));

        let reply = ask(&mut board, json!({"op": "clear"}));

        assert_eq!(reply, json!({"ok": true}));
        assert!(elements(&mut board).is_empty());
        // Every layer is gone, the connector's label too, in one step.
        assert_eq!(layers_on_page(&board), 0);
        assert_eq!(board.session.undo_depth(), (steps + 1, 0));
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(layers_on_page(&board), layers);
        assert_eq!(elements(&mut board).len(), 4);
        assert_eq!(board.session.undo_depth(), (steps, 1));
    }

    #[test]
    fn clear_of_an_empty_page_changes_nothing_and_only_that_page_is_cleared() {
        let mut board = board();
        let before = board.doc().unwrap();
        // The selection is empty too, which is what the engine's delete would fall back to.
        assert_eq!(ask(&mut board, json!({"op": "clear"})), json!({"ok": true}));
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (0, 0));

        add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
        board.execute("page.add", &json!({})).unwrap();
        add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
        ask(&mut board, json!({"op": "clear"}));
        assert!(elements(&mut board).is_empty());
        board.execute("page.select", &json!({"index": 0})).unwrap();
        assert_eq!(elements(&mut board).len(), 1);
    }

    /// A board file of two pages, a sticky on each.
    fn two_page_file(dir: &std::path::Path) -> String {
        let mut other = BoardDoc::new_board("Two pages").unwrap();
        other
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "first"}))
            .unwrap();
        other
            .execute("page.add", &json!({"name": "Second"}))
            .unwrap();
        other
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "second"}))
            .unwrap();
        let path = dir.join("two.figcraft");
        std::fs::write(&path, other.to_bytes().unwrap()).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn replace_loads_a_figcraft_file_as_one_undo_step() {
        let dir = tempfile::tempdir().unwrap();
        let other = other_board_file(dir.path());
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        // Two pages of its own: both are replaced.
        board
            .execute("page.add", &json!({"name": "Notes"}))
            .unwrap();
        board.execute("page.select", &json!({"index": 0})).unwrap();
        let (uid, steps) = (board.session.ds().unwrap().uid, board.session.undo_depth());

        let reply = ask(&mut board, json!({"op": "replace", "path": other}));

        // The canvas shows the file's page, and says what is on it as `get` does.
        let now = elements(&mut board);
        assert_eq!(now.len(), 2, "{now:?}");
        assert!(now.iter().all(|e| e["text"] == json!("other")), "{now:?}");
        assert_eq!(reply["elements"], json!(now));
        assert_eq!(board.doc().unwrap().pages.len(), 1);
        assert!(board.session.selection().is_empty());
        // In the canvas's own session: one document, the one it had, and still a board.
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(board.session.ds().unwrap().uid, uid);
        assert_eq!(board.kind(), DocKind::Board);
        // One step: undo brings the old board back, pages and all, and redo the file's.
        assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
        assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(board.doc().unwrap().pages.len(), 2);
        let back = elements(&mut board);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0]["text"], json!("mine"));
        ask(&mut board, json!({"op": "command", "command": "edit.redo"}));
        assert_eq!(elements(&mut board).len(), 2);
    }

    #[test]
    fn replace_takes_every_page_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = two_page_file(dir.path());
        let mut board = board();
        add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));

        ask(&mut board, json!({"op": "replace", "path": path}));

        // The first page is on show, and the old page is gone.
        let doc = board.doc().unwrap();
        let names: Vec<&str> = doc
            .pages
            .iter()
            .map(|page| doc.node(*page).unwrap().name.as_str())
            .collect();
        assert_eq!(names, ["Page 1", "Second"]);
        let now = elements(&mut board);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0]["text"], json!("first"));
    }

    #[test]
    fn replace_keeps_a_design_file_a_board() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = design();
        file.execute(
            "node.create",
            &json!({"type": "frame", "x": 0, "y": 0, "name": "Screen"}),
        )
        .unwrap();
        let path = dir.path().join("design.figcraft");
        std::fs::write(&path, file.to_bytes().unwrap()).unwrap();
        let mut board = board();

        let reply = ask(&mut board, json!({"op": "replace", "path": path}));

        assert_eq!(board.kind(), DocKind::Board);
        assert_eq!(reply["elements"][0]["name"], json!("Screen"));
        assert_eq!(reply["elements"][0]["type"], json!("frame"));
    }

    #[test]
    fn replace_of_a_missing_or_bad_file_is_an_error_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.figcraft");
        std::fs::write(&bad, b"not a figcraft file").unwrap();
        let missing = dir.path().join("missing.figcraft");
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let (before, steps) = (board.doc().unwrap(), board.session.undo_depth());

        let error = refusal(&mut board, json!({"op": "replace", "path": missing}));
        assert!(
            error.starts_with("replace:") && error.contains("missing.figcraft"),
            "{error}"
        );
        let error = refusal(&mut board, json!({"op": "replace", "path": bad}));
        assert!(error.starts_with("replace:"), "{error}");

        // Not a new, empty board over the canvas: it is as it was.
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), steps);
        assert_eq!(elements(&mut board).len(), 1);
    }

    /// The scene of the Excalidraw fixture: nine layers on the page, and an embed it leaves out.
    fn scene() -> Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/excalidraw/basic.excalidraw"
        ))
        .unwrap()
    }

    #[test]
    fn replace_loads_an_excalidraw_scene_as_one_undo_step() {
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        // Two pages of its own: both are replaced.
        board
            .execute("page.add", &json!({"name": "Notes"}))
            .unwrap();
        board.execute("page.select", &json!({"index": 0})).unwrap();
        let (uid, steps) = (board.session.ds().unwrap().uid, board.session.undo_depth());

        let reply = ask(&mut board, json!({"op": "replace", "scene": scene()}));

        // The canvas shows the scene, and says what is on it as `get` does, and what was left out.
        let now = elements(&mut board);
        assert_eq!(now.len(), 9, "{now:?}");
        assert_eq!(reply["elements"], json!(now));
        assert_eq!(reply["notes"], json!(["skipped 1 embed"]));
        assert_eq!(board.doc().unwrap().pages.len(), 1);
        assert!(board.session.selection().is_empty());
        // In the canvas's own session: one document, the one it had, and still a board.
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(board.session.ds().unwrap().uid, uid);
        assert_eq!(board.kind(), DocKind::Board);
        // One step: undo brings the old board back, pages and all, and redo the scene.
        assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
        assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(board.doc().unwrap().pages.len(), 2);
        let back = elements(&mut board);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0]["text"], json!("mine"));
        ask(&mut board, json!({"op": "command", "command": "edit.redo"}));
        assert_eq!(elements(&mut board).len(), 9);
    }

    #[test]
    fn a_replaced_scene_keeps_what_it_joins_and_the_ids_it_had() {
        let mut board = board();
        let reply = ask(&mut board, json!({"op": "replace", "scene": scene()}));
        let now = reply["elements"].as_array().unwrap();
        let of = |kind: &str| now.iter().find(|e| e["type"] == json!(kind)).unwrap();
        // The arrow still joins the rectangle and the ellipse, which are in the new canvas.
        let arrow = now.iter().find(|e| e.get("from").is_some()).unwrap();
        assert_eq!(arrow["from"], of("rectangle")["id"]);
        assert_eq!(arrow["to"], of("ellipse")["id"]);
        // The picture and the section came through the engine's import.
        assert_eq!(of("image")["width"], json!(120.0));
        assert_eq!(of("section")["name"], json!("Group A"));
        let doc = board.doc().unwrap();
        let section = doc
            .node(of("section")["id"].as_str().unwrap().parse().unwrap())
            .unwrap();
        assert_eq!(section.children.len(), 1);
        // The ids the elements had in Excalidraw name them still.
        let rectangle = of("rectangle")["id"].as_str().unwrap().to_string();
        let reply = ask(
            &mut board,
            json!({"op": "update", "elements": [{"id": "box", "text": "Renamed"}]}),
        );
        assert_eq!(reply["updated"], json!(1), "{reply}");
        assert_eq!(listed(&mut board, &rectangle)["text"], json!("Renamed"));
    }

    #[test]
    fn replace_loads_an_excalidraw_file() {
        let dir = tempfile::tempdir().unwrap();
        let text = include_str!("../tests/fixtures/excalidraw/basic.excalidraw");
        for name in ["board.excalidraw", "BOARD.Excalidraw"] {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            let mut board = board();
            add(
                &mut board,
                json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
            );

            let reply = ask(&mut board, json!({"op": "replace", "path": path}));

            assert_eq!(reply["elements"].as_array().unwrap().len(), 9, "{name}");
            assert_eq!(reply["notes"], json!(["skipped 1 embed"]));
            assert_eq!(elements(&mut board).len(), 9);
            assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
            // The file is read, never written.
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn a_scene_wins_over_a_path_and_an_empty_one_clears_the_canvas() {
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let reply = ask(
            &mut board,
            json!({"op": "replace", "scene": {"elements": []}, "path": "/nowhere/a.figcraft"}),
        );
        assert_eq!(reply, json!({"ok": true, "elements": []}));
        assert!(elements(&mut board).is_empty());
        ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
        assert_eq!(elements(&mut board).len(), 1);
        // `null` is no scene: the path is what counts.
        let error = refusal(
            &mut board,
            json!({"op": "replace", "scene": null, "path": "/nowhere/a.figcraft"}),
        );
        assert!(error.contains("a.figcraft"), "{error}");
    }

    #[test]
    fn replace_of_a_bad_scene_or_excalidraw_file_is_an_error_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            path.to_string_lossy().into_owned()
        };
        let not_json = write("not-json.excalidraw", "{ not json");
        let no_elements = write("no-elements.excalidraw", r#"{"type": "excalidraw"}"#);
        let missing = dir.path().join("missing.excalidraw");
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let (before, steps) = (board.doc().unwrap(), board.session.undo_depth());

        for (op, says) in [
            (json!({"op": "replace", "scene": {}}), "`elements`"),
            (json!({"op": "replace", "scene": [1, 2]}), "`elements`"),
            (json!({"op": "replace", "scene": "scene"}), "`elements`"),
            (
                json!({"op": "replace", "path": missing}),
                "missing.excalidraw",
            ),
            (
                json!({"op": "replace", "path": not_json}),
                "isn't valid JSON",
            ),
            (json!({"op": "replace", "path": no_elements}), "`elements`"),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.starts_with("replace: "), "{op}: {error}");
            assert!(error.contains(says), "{op}: {error}");
        }
        // The file that is no scene is named, not only said to be one.
        let error = refusal(&mut board, json!({"op": "replace", "path": no_elements}));
        assert!(error.contains("no-elements.excalidraw"), "{error}");

        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), steps);
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn replace_needs_a_scene_or_a_file_of_a_kind_it_knows() {
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let before = board.doc().unwrap();
        // No scene and no path, or a path that isn't text, runs nothing.
        for op in [
            json!({"op": "replace"}),
            json!({"op": "replace", "path": 5}),
            json!({"op": "replace", "scene": null}),
        ] {
            let error = refusal(&mut board, op.clone());
            assert!(error.starts_with("replace needs"), "{op}: {error}");
        }
        for path in ["/tmp/board.png", "/tmp/board.figcraft.bak", "/tmp/board"] {
            let error = refusal(&mut board, json!({"op": "replace", "path": path}));
            let expected = format!("replace: {path} is not a .figcraft or .excalidraw file");
            assert_eq!(error, expected);
        }
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
        assert_eq!(board.session.undo_depth(), (1, 0));
    }

    /// Every id `commands` lists, with the whole reply checked.
    fn listed_commands(board: &mut BoardDoc) -> Vec<Value> {
        let reply = ask(board, json!({"op": "commands"}));
        reply["commands"].as_array().expect("commands").clone()
    }

    #[test]
    fn commands_hide_hidden_ids() {
        let mut board = board();
        let commands = listed_commands(&mut board);
        let ids: Vec<&str> = commands.iter().map(|c| c["id"].as_str().unwrap()).collect();

        // Nothing the canvas hides: no file.new, nor any other hidden id, nor a plugin's.
        assert!(!ids.contains(&"file.new"));
        assert!(!ids.contains(&"plugins.manage"));
        for id in &ids {
            assert!(!policy::is_hidden(id), "{id} is hidden");
        }
        // Saving is Sawit's: `command` refuses it, so it isn't offered (P7-C11).
        assert!(!ids.contains(&"file.save"));
        assert!(!ids.contains(&"file.saveAs"));

        // What an agent can do is there, as {id, label, params}.
        let sticky = commands
            .iter()
            .find(|c| c["id"] == json!("board.sticky"))
            .unwrap();
        assert_eq!(sticky["label"], json!("Sticky note"));
        assert!(
            sticky["params"].as_str().unwrap().contains("text?"),
            "{sticky}"
        );
        for command in &commands {
            let keys: Vec<&String> = command.as_object().unwrap().keys().collect();
            assert_eq!(keys, ["id", "label", "params"], "{command}");
            assert!(command.as_object().unwrap().values().all(Value::is_string));
        }
        assert!(ids.len() > 200, "{} commands", ids.len());
    }

    #[test]
    fn commands_lists_only_what_the_command_op_runs() {
        let mut board = board();
        let commands = listed_commands(&mut board);
        let ids: Vec<&str> = commands.iter().map(|c| c["id"].as_str().unwrap()).collect();

        // The one rule: nothing is offered that `command` refuses, up front or by the guard that
        // undoes a command that opens a document.
        for id in &ids {
            assert_eq!(
                refused(id, &Value::Null),
                None,
                "{id} is listed but refused"
            );
            assert!(!OPENS_DOCUMENTS.contains(id), "{id} opens a document");
        }
        for id in HANDLED_BY_SAWIT.iter().chain(OPENS_DOCUMENTS) {
            assert!(!ids.contains(id), "{id} is listed");
        }
        // Every other command of the engine is, except what the policy hides.
        let engine = figcraft_engine::cmd::all_commands();
        let unlisted: Vec<&str> = engine
            .iter()
            .map(|c| c.id)
            .filter(|id| !ids.contains(id))
            .collect();
        for id in &unlisted {
            assert!(
                policy::is_hidden(id)
                    || HANDLED_BY_SAWIT.contains(id)
                    || OPENS_DOCUMENTS.contains(id),
                "{id} is not listed, and nothing says why"
            );
        }
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "an id is listed twice");
    }

    #[test]
    fn commands_that_open_documents_are_real_engine_ids() {
        // A rename in the engine would leave a stale id here, and the command in the list.
        for id in OPENS_DOCUMENTS {
            assert!(figcraft_engine::cmd::find_command(id).is_some(), "{id}");
        }
    }

    #[test]
    fn without_a_document_ops_answer_with_an_error() {
        let mut board = BoardDoc {
            session: figcraft_engine::Session::empty(),
        };
        for op in [
            json!({"op": "get"}),
            json!({"op": "add", "elements": []}),
            json!({"op": "update", "elements": []}),
            json!({"op": "delete", "ids": []}),
            json!({"op": "clear"}),
            json!({"op": "replace", "path": "/tmp/canvas.figcraft"}),
            json!({"op": "replace", "scene": {"elements": []}}),
            json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}}),
            json!({"op": "pointer", "kind": "down", "x": 0, "y": 0}),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            assert!(reply["error"].as_str().is_some_and(|e| !e.is_empty()));
        }
    }

    #[test]
    fn unknown_op_is_reported() {
        let mut board = board();
        let reply = run(&mut board, &json!({"op": "teleport", "to": [0, 0]}));
        assert_eq!(
            reply,
            json!({"ok": false, "error": "teleport is not supported yet"})
        );
        // An op with no name, or that isn't an op at all.
        for op in [json!({}), json!("get"), json!({"op": 3}), json!(null)] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            assert!(reply["error"].as_str().is_some_and(|e| e.contains("op")));
        }
    }

    /// Sends pointer op `kind` at page point (`x`, `y`), checks that it answered, and returns the
    /// answer.
    fn pointer_at(board: &mut BoardDoc, kind: &str, x: f64, y: f64) -> Value {
        let reply = run(
            board,
            &json!({"op": "pointer", "kind": kind, "x": x, "y": y}),
        );
        assert_eq!(reply["ok"], json!(true), "{reply}");
        reply
    }

    #[test]
    fn pointer_op_drives_tools() {
        let mut board = board();
        let reply = send(&mut board, "tool.sticky", json!({}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        pointer_at(&mut board, "down", 0.0, 0.0);
        let reply = pointer_at(&mut board, "up", 0.0, 0.0);
        assert_eq!(elements(&mut board).len(), 1);
        // The answer says which tool is on, back to Move once the sticky is placed, and what is
        // selected, by id.
        assert_eq!(reply["tool"], json!("move"));
        let selection = reply["selection"].as_array().expect("a selection");
        assert_eq!(selection.len(), 1, "{reply}");
        assert!(
            selection[0]
                .as_str()
                .is_some_and(|id| id.parse::<u64>().is_ok()),
            "{reply}"
        );
    }

    #[test]
    fn pointer_op_takes_page_points_whatever_the_view() {
        let mut board = design();
        // Zoomed in and panned: the op's points are page points all the same.
        board.session.ds_mut().unwrap().view = figcraft_engine::ViewState {
            cx: 37.0,
            cy: -12.0,
            zoom: 2.0,
            vw: 640.0,
            vh: 480.0,
            ..figcraft_engine::ViewState::default()
        };
        send(&mut board, "tool.rectangle", json!({}));
        for (kind, x, y) in [
            ("down", 10.0, 20.0),
            ("move", 60.0, 60.0),
            ("move", 110.0, 100.0),
            ("up", 110.0, 100.0),
        ] {
            pointer_at(&mut board, kind, x, y);
        }
        let elements = elements(&mut board);
        assert_eq!(elements.len(), 1, "{elements:?}");
        let drawn = &elements[0];
        let bounds = ["x", "y", "width", "height"].map(|key| drawn[key].as_f64().unwrap());
        assert_eq!(bounds, [10.0, 20.0, 100.0, 80.0], "{drawn}");
    }

    #[test]
    fn pointer_op_leaves_the_views_requests_for_it() {
        let mut board = design();
        let made = board
            .execute(
                "node.create",
                &json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100}),
            )
            .unwrap();
        let frame = figcraft_engine::params::id_of(&made["id"]).unwrap();
        board.execute("select.none", &json!({})).unwrap();
        // The view hands the engine its frames' names as it paints them: this one just above the
        // frame's top-left corner.
        let corner = figcraft_geom::Point::new;
        board.session.tool.labels = vec![(
            frame,
            [
                corner(0.0, -20.0),
                corner(60.0, -20.0),
                corner(60.0, -4.0),
                corner(0.0, -4.0),
            ],
        )];

        let reply = pointer_at(&mut board, "double", 10.0, -12.0);

        // Renaming the frame there is the view's to do: the request waits for it.
        let kinds: Vec<&str> = (board.session.ui_requests.iter())
            .map(|request| request.kind.as_str())
            .collect();
        assert_eq!(kinds, ["canvas.renameLabel"]);
        assert_eq!(reply["selection"], json!([frame.to_string()]));
    }

    #[test]
    fn malformed_pointer_ops_are_reported() {
        let mut board = board();
        send(&mut board, "tool.sticky", json!({}));
        for (op, says) in [
            (json!({"op": "pointer", "x": 0, "y": 0}), "kind"),
            (json!({"op": "pointer", "kind": 1, "x": 0, "y": 0}), "kind"),
            (
                json!({"op": "pointer", "kind": "tap", "x": 0, "y": 0}),
                "tap",
            ),
            (json!({"op": "pointer", "kind": "down"}), "x and y"),
            (
                json!({"op": "pointer", "kind": "down", "x": "0", "y": 0}),
                "x and y",
            ),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains(says), "{op}: {error}");
        }
        // None of them reached the tools: no sticky was placed, and the tool is still on.
        assert!(elements(&mut board).is_empty());
        assert_eq!(board.session.tool.tool.id(), "sticky");
    }

    /// Sends key op `key` with the modifier keys in `held` (`{"cmd": true}`), checks that it
    /// answered, and returns whether the canvas took the key.
    fn press(board: &mut BoardDoc, key: &str, held: Value) -> bool {
        let mut op = json!({"op": "key", "key": key});
        for (modifier, on) in held.as_object().expect("modifier keys") {
            op[modifier] = on.clone();
        }
        let reply = run(board, &op);
        assert_eq!(reply["ok"], json!(true), "{op}: {reply}");
        reply["handled"].as_bool().expect("handled")
    }

    #[test]
    fn key_op_routes_like_the_keyboard() {
        let mut board = board();
        let reply = run(&mut board, &json!({"op": "key", "key": "r"}));
        assert_eq!(reply, json!({"ok": true, "handled": true}));
        assert_eq!(board.session.tool.tool.id(), "rectangle");
        // With the modifier keys held: ⌘Z takes back the sticky.
        send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        assert!(press(&mut board, "z", json!({"cmd": true})));
        assert!(elements(&mut board).is_empty());
        // A key that is no shortcut, and a hidden command's (⌘N is file.new), aren't the canvas's.
        assert!(!press(&mut board, "g", json!({})));
        assert!(!press(&mut board, "n", json!({"cmd": true})));
        assert_eq!(board.session.docs.len(), 1);
        // The shortcut of a command Sawit carries out itself is the canvas's, though the op runs
        // nothing for it (⌘S, the save).
        assert!(press(&mut board, "s", json!({"cmd": true})));
    }

    #[test]
    fn key_op_leaves_the_views_requests_for_it() {
        let mut board = board();
        // ⌘K asks the view for the Actions palette: the request waits for it.
        assert!(press(&mut board, "k", json!({"cmd": true})));
        let kinds: Vec<&str> = (board.session.ui_requests.iter())
            .map(|request| request.kind.as_str())
            .collect();
        assert_eq!(kinds, ["palette.actions"]);
        board.session.ui_requests.clear();

        // ⇧⌘↩ while typing in a sticky makes the next one, and typing goes on there, as from the
        // keyboard: the op does what the engine asked of the view that needs no view.
        let made = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        let first = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
        assert!(figcraft_engine::board::edit_text(&mut board.session, first).unwrap());
        assert!(press(
            &mut board,
            "Enter",
            json!({"shift": true, "cmd": true})
        ));
        assert!(board.session.ui_requests.is_empty());
        let ds = board.session.ds().unwrap();
        let layers = &ds.doc.node(ds.page).unwrap().children;
        let [one, next] = layers[..] else {
            panic!("layers {layers:?}")
        };
        assert_eq!(one, first);
        let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
        assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
    }

    #[test]
    fn malformed_key_ops_and_refusals_are_reported() {
        let mut board = board();
        for op in [
            json!({"op": "key"}),
            json!({"op": "key", "key": 5}),
            json!({"op": "key", "key": ""}),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains("a key op needs a key"), "{op}: {error}");
        }
        // A shortcut the engine refuses: ⌘G groups the selection, and nothing is selected.
        let reply = run(&mut board, &json!({"op": "key", "key": "g", "cmd": true}));
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("nothing is selected"), "{error}");
    }
}
